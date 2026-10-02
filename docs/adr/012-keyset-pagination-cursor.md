# ADR-012：反熵分页改用主键游标，闭合差量缩短导致的尾部丢失

状态：已接受
日期：2026-10-02
相关：ADR-010、ADR-011

## 背景

ADR-011 声称闭合了跨节点经验同步的第三个丢失窗口。补测「补齐信号在分页在途时被延后」这条路径时，暴露了一个与 `Lagged` 无关的既有缺陷，它属于 ADR-010。

先说清楚这不是 ADR-011 引入的：把洪泛关掉、纯走正常初始反熵，缺陷同样复现。

## 现象

A 持有 263 条，B 加入后走正常反熵翻页，停在 **256** 条，恰好缺 `2 * SYNC_PAGE_SIZE` 之后的 7 条（`SYNC_PAGE_SIZE = 128`）。

## 根因

服务端每一页都用 **dial 方当前**的摘要重算差量：

```
delta = missing_keys(B 的当前摘要, A 的全部键)
```

B 每收到一页就多持有一批行，于是下一页的差量随之**缩短**并整体**前移**。实测 `delta_total` 逐页为 `263 → 263 → 253`。

致命处在于游标：`offset` 是对「一个会随时间缩短、且头部被删除的列表」取下标。列表缩短并前移后，按旧下标切片必然跳过一批仍缺失的行。

```
第 1 页：delta 263 条，取 delta[0..128]   → next_offset = 128
第 2 页：B 已持有前 128 条，delta 缩为 135 条并前移
         取 delta[128..256]               → 越界，实际只取到 7 条
第 3 页：delta 253 条，取 delta[256..]    → 空页，more = false
```

第 3 页 `page_len = 0` 且 `more = false`，拨号器判定「同步完成」，那 7 行永久缺失且无任何机制会再提。

关键在于差量跨**三页及以上**才触发：只跨两页时缩短量不足以把内容挤出切片，所以既有用例全都测不到。

## 为什么既有用例漏掉了

`anti_entropy_integration` 的 `drain_sync` 每轮传同一个静态 `dialer` 摘要，从不反映「dial 方已经收到了什么」。于是差量长度在整个循环中恒定，下标游标在测试里**恰好正确**。

缺陷只在「dial 方真的把收到的行写进库里、下一轮摘要随之更新」时暴露——也就是生产里的实际形态。测试替身与真实行为不一致，是这类 bug 逃逸的常见原因。

## 决策

游标从**下标**改为**内容**：记录最后一个已送达的主键，服务端取严格大于它的下一段。

差量如何缩短只影响「还剩多少条」，不影响「从哪一条继续」。

- `SyncRequest.offset` → `SyncRequest.cursor`（新增字段 4，`offset` 保留字段号并标记 deprecated）
- `SyncPageEnd.next_offset` → `next_cursor`（同上）
- 服务端 `delta.partition_point(|k| *k <= cursor)` 二分定位
- 游标取**请求区间**的最后一个差量主键，而非实际发出的最后一条：末条被 `build_sync_page` 过滤时，后者会把游标留在段内导致重复发送

兼容老节点：老节点只发 `offset`，服务端一律按空游标从头回送。合并路径幂等，多发一轮只浪费带宽，不会造成错漏。

### 混合版本：为什么两边都能收敛

这是本 ADR 最需要写清楚的部分，因为它不成立就会造成无限翻页。

| 组合 | 行为 | 结果 |
|---|---|---|
| 新 dialer ↔ 旧 server | 旧 server 不解释 `cursor`，恒发第一页、`next_cursor` 为空 | 收敛，但带宽 O(n²) |
| 旧 dialer ↔ 新 server | 新 server 恒置 `next_offset = 0`，旧 dialer 停在第一页 | 收敛，但带宽 O(n²) |
| 新 ↔ 新 | 游标推进 | 线性，一页一段 |

两个方向的「每轮都从第一页重来」之所以不会无限循环，靠的是**摘要随持有量增长而使差量缩短**：

```
20 条差量、页大小 8：
第 1 轮：差量 20，取前 8 → 持有 8
第 2 轮：差量 12，取前 8 → 持有 16
第 3 轮：差量 4，取 4 → more = false
```

差量严格递减，必然终止。

值得注意的是：**这恰好是 ADR-010 用下标游标时不成立的前提**。那里论证「无状态分页」时默认了「同一轮内差量恒定」，而真实 dial 方每轮都更新摘要。这个错误的前提正是尾部丢失的根源。

因此混合版本的正确性依赖于一条既有不变量：**dial 方每页后必须真正把收到的行写进库并更新摘要**。若将来引入批量写、延迟提交或缓存摘要，两个方向都会同时退化成无限翻页。已用变异测试锁住这一点。

### 测试的边界

`mixed_version_old_dialer_new_server_still_converges` 的名字容易读成「验证了旧 dialer 与新 server 互通」。**它没有**——它和场景 A 跑的是同一段新代码，真正的旧 proto 解析器已被替换掉，无法在进程内构造。两个用例的实际作用是锁住「服务端忽略旧字段时仍收敛」这条前提。

## 顺带修正：补齐信号被失败吞掉

审计 ADR-011 时发现一个独立的缺陷：`resume_pending_catchup` 把 `*pending = false`
写在**发请求之前**，于是摘要导出失败或出站通道关闭时，信号被静默丢弃。

失败原因通常是暂时的（通道抖动、数据库短暂不可用），而信号一旦丢失就只能等
下一次 `Lagged` 或重连，中间可能间隔很久。

修正：只在请求**真的发出去**之后才清位；失败则保留标志等待重试。没有经验库
时仍清位（补齐无从谈起，留着只会空转）。

用例 `deferred_catchup_survives_failed_send` 用一个已关闭的通道确定性地制造失败，
断言失败两次后标志仍为真，换可用通道后被兑现并开启分页会话。变异测试（把清位
挪回请求之前）被该用例捕获。

## 一个断言通过的用例，实际零覆盖

`catchup_deferred_while_paging_converges_exactly` 的名字宣称它覆盖「补齐信号在分页在途时被延后」。提交 ADR-012 后回头做变异，发现它经不起检验：

把 `catchup_pending = true` 改成 `drop(catchup_pending)`（即延后改为丢弃，正是被测的缺陷），用例**仍然通过**。

装 tracing subscriber 抓日志后原因清楚（下面是旧版用例的真实时间线）：

```
04.779  WARN  lagged（×21）          ← 洪泛立刻压垮通道，信号全部发完
04.811  INFO  Join successful         ← 拨号器建连
04.822  INFO  sync request sent       ← 拨号器才开始工作
04.843  INFO  replied sent=128 more=true
04.862  INFO  sync request sent
04.889  INFO  replied sent=128 more=true
04.907  INFO  sync request sent
04.917  INFO  replied sent=7 more=false   ← 分页已收尾
```

洪泛是**同步**写入的，比拨号器建连早约 11ms 把 21 个信号全发完了。

这里要修正一版早先的错误归因。我当时写的是「信号全部落在 `paging = None` 上，
走的是 `None` 分支」——这不对。装上日志实测两个分支各出现过 0 次：
`None` 分支的 `anti-entropy: lagged catch-up requested` 和 `Some` 分支的
`anti-entropy: catch-up deferred until paging finishes` 都是零条。
信号根本没进拨号器的接收端。

机制是 `broadcast::Receiver::resubscribe`：`run_stream` 每次（重）连接都用
它拿一个新的接收器，而它**只从订阅那一刻开始计数**，看不到订阅之前已发出的信号。
实测对照（`sisyphus-bridge` 内临时探针，事后删除）：

| 时序 | `send()` | `try_recv()` |
|------|----------|--------------|
| 先发后 `resubscribe`（实际发生的） | `Ok`（无错） | `Empty`（信号丢失） |
| 先 `resubscribe` 后发 | `Ok` | `Ok`（收到） |

也就是说**发送成功不等于信号被消费**：`tx.send(())` 返回 `Ok`，因为上游那个
`observer` 本身就是订阅者。于是服务端日志里的 `catchup_signalled=true` 会
误导人——它只说明 `catchup_tx.is_some()`，不说明有人听见了。真正的丢弃发生在
接收端，而且不报错、无日志。

顺带一提，这条路径此前还被 ADR-011 记录为「用洪泛制造信号」就够了。既有断言「信号确实触发过」只证明 Lagged 发生了，完全不能证明信号被消费、更不能证明信号撞上的是分页在途的时刻。

这个缺口本身不致命：`run_stream` 在每次流建立时无条件发一次全量反熵拉取
（ADR-010 引入），建连瞬间错过的信号由它兜住，最终仍然收敛。致命的是它
**伪装成覆盖**——用例照样通过、断言照样成立，却什么都没验到。

修正：先轮询等 B 真正收到第一页，那一刻拨号器已 `resubscribe` 且 `paging = Some` 必然成立，再洪泛；差量同时从 3 页加到 7 页以拉宽时间窗。

修正后：M1 变异被捕获，缺失键正是被丢弃那一段（`exp:...:40` 起）；连续 5 次运行稳定通过（2.5–4.7 秒）。

教训是通用的：**「用例通过」不等于「分支被覆盖」**。当竞态是测试要验证的对象本身时，必须用变异测试反查分支是否真的被执行，而不是看断言是否变红。

## 代价

游标从 8 字节变成最长约 40 字节的字符串，`SyncRequest` 体积略增。相对「静默丢数据」完全可接受。

## 验证

`anti_entropy_integration` 8/8、`cluster_integration` 18/18、`sisyphus-types` 75/75、`make test-all` 全通过。

新增 `paging_survives_delta_shrinking_between_requests`：每收到一页就真正写入 dial 方，下一轮用**更新后的**摘要，复现差量缩短的真实形态。

变异测试：

| 注入的缺陷 | 结果 |
|---|---|
| 游标退回 ADR-010 的下标语义（`cursor.parse()` 当下标，回传已发条数） | ✅ 捕获 3 例，缺失键恰为尾部 9..16，与生产观测到的形态一致 |
| `partition_point` 比较差一（`<` 而非 `<=`） | ✅ 捕获 2 例 |
| 忽略 dial 方摘要，恒按全量算差量 | ✅ **仅**两个混合版本用例捕获（8 passed / 2 failed）|

最后一行值得强调：这条变异杀掉了其他所有测试，两个混合版本用例是仅有的拦截者。它们不是装饰。

### 三次错误断言：一条防御其实无法构造测试

我想为「游标取区间末位而非实际发出末条」补一条测试，写了三次、错了三次：

1. 我以为 `build_sync_page` 会把「`local` 已持有」的行从 `rows` 里滤掉。它不会。`build_sync_page` 用 `local` 重算的是**差量集合**，而差量集合按定义就不包含 `local` 已持有的键——不会因为「已持有」而被滤。
2. 我以为序列化会因非有限浮点失败，于是想造一条坏数据。实测 `serde_json` 把 `NaN`/`inf` 写成 `null`，`to_vec` 从不失败。
3. 我改用「键不在 `rows` 快照内」来构造收窄。同样失败：`build_sync_page` 的差量正是**从 `rows` 本身**算出的，它不可能发出一个自己快照里没有的键。

结论：这个收窄唯一真实的发生处是 `serve_sync_request` 内部——`page_keys` 算完之后、`load_by_keys` 执行**之前**发生的并发删除。要注入它，必须让 `load_by_keys` 本身可替换或可控延迟，而我既没有为此加抽象，也不认为为了给一条防御补测试而改动生产结构是值得的。

因此我**删掉了这条测试**，而不是保留一个通过的空壳。现状记为：`build_sync_page` 的收窄分支无测试保护，游标防御依赖代码审阅而非变异捕获。三次失败本身是有价值的教训——前两次假设都出于「想当然地以为这里会过滤」。

同理，既然 `experiences.len()` 与 `page_keys.len()` 在现有可观测路径下从不不同，这条防御实际是针对一个尚无法复现的场景的冗余。它无害（区间末位在无收窄时与发出末位完全相等），保留即可。

## 遗留

- **旧格式主键仍无法补齐**：`exp:{timestamp}` 解析不出 origin/counter。这是 ADR-010 之前的历史数据，已记录的取舍。
- **`build_sync_page` 的收窄分支无测试保护**（见上）：需在 `serve_sync_request` 内注入可控并发删除才能验证游标防御。
- **混合版本未经真实跨版本验证**：两个用例都只跑新代码。它们锁住的是收敛前提，不是互通行为。
- **摘要不变是混合版本正确性的前提**：若 dial 方改为延迟提交或缓存摘要，两个方向会同时退化为无限翻页。建议后续重构时保留这两条用例。
- **游标比较使用字节序**：依赖主键按字节序排序（`sorted_missing_keys`）。若未来主键格式变为非字典序，需重新评估。
- **没有显式的周期 reconcile**：保留的补齐信号靠下一次 `Lagged` 或会话收尾兑现。若某节点此后**再未发生 Lagged**，一次失败的重试可能被无限期推迟。当前由重连兜底（重连必然重发初始反熵），但这依赖重连真的发生。

## References

- ADR-010 § 反熵补偿（分页游标设计与「无状态分页」的原始论证）
- ADR-011 § 遗留
- `crates/sisyphus-bridge/src/peer/anti_entropy.rs` — `serve_sync_request`、`SyncPage`
- `crates/sisyphus-bridge/src/peer/fallback.rs` — `PagingCursor`
- `crates/sisyphus-bridge-api/proto/cluster.proto` — `SyncRequest.cursor`、`SyncPageEnd.next_cursor`
- `crates/sisyphus-bridge/tests/anti_entropy_integration.rs` — `paging_survives_delta_shrinking_between_requests`