# ADR-010: 跨节点经验的反熵补偿（重连补齐）

**Status**: Implemented (2026-10-02) — 选项 A 已实施并通过验证
**Deciders**: core team
**Supersedes**: 无（补充 ADR-009 Risk 2 未实施的缓解措施）
**Related**: ADR-009 § 2 Experience CRDT、§ 1 peer 协议

## Context

2026-10-02 的核查发现，跨节点经验同步此前从未在生产路径生效：事件从本地广播到对端落库要穿过 gRPC 流、proto 解码、本地事件总线三段，每段各有一处静默丢弃，其中一处是 `fallback.rs` 复制自 `service.rs` 却仍在丢弃的第二份实现。三处已修复，双节点端到端用例已补齐（10/10 通过）。

链路通了之后，一个**在 ADR-009 里被识别但从未处理**的问题浮出水面：`ExperienceSync` 是即发即失的。

### 三个丢失窗口

1. **新节点加入**：`Join` 只交换 peer 列表与 leader，不携带任何经验。新节点对既有经验一无所知，直到恰好有新的经验产生并广播。（TODO 9.1.1「启动时全量 sync」）
2. **peer 断连期间**：断连期间其他节点产生的经验永久丢失。重连只恢复流，不补历史。
3. **订阅者落后**：`ExperienceSyncHandler` 对 `RecvError::Lagged` 的处理是 `continue`，丢多少不知道、也不补。

### ADR-009 原定的缓解措施与其问题

ADR-009 § Risk 2 写的是「启动时强制全量 sync（每个 peer 发送本地所有 exp）」。该措施有两个未处理的问题：

- **无界突发**：经验数量随运行时间单调增长，一次全量推送的体积没有上界。
- **O(N²)**：N 个节点两两互推全量。

同时需要更正 ADR-009 的 Status：它写着 § 2 Experience CRDT「已实施」。就 2026-10-02 的核查而言，准确表述是「CRDT 数据结构与合并算法已实现，但未被任何生产路径调用」——即本 ADR 之前，同步链路是断的。

### 约束

- **不得阻塞写入路径**：本地 `record()` 不能等待网络。经验同步是尽力而为的旁路。
- **幂等**：补齐走的是同一条 `ExperienceSync` → `apply_remote` → `merge_crdt` 路径，已天然幂等（返回 `Applied` / `Kept` / `Equal`）。
- **有界**：单次响应的条目数必须有上界。

### 前置缺陷 P1：经验行标识会碰撞（2026-10-02 发现并已修复）

**已修复**。修复分两层，第二层是实施中才发现的：

**第一层 · 主键碰撞**（原始症状）

- 经验行的主键是 `id = format!("exp:{}", exp.timestamp)`，而 `timestamp` 取自 `SystemTime::now().duration_since(UNIX_EPOCH).as_secs()`——**秒级精度**。
- `record_experience` 用的是裸 `INSERT`，没有 `OR REPLACE` / `ON CONFLICT`。
- 实测：同一秒内写入两条经验，第二条返回 `Err("Failed to record experience")`，第一条完好。
- `execute_and_record` 对该错误只 `tracing::warn!` 后继续，**丢失是静默的**，不影响目标执行结果。

修复：主键改为 `exp:{timestamp}:{origin}:{counter}`（见 `experience_store_id`）。`origin` 与 `counter`
在 `bump_local_clock` 之后已存在于 `vector_clock` 中，**不新增字段，因此不改变跨节点序列化格式**。
接收端采用发送方随 `ExperienceSync` 传来的主键，不再按本地 timestamp 重算——否则同一行会对端本地各存一份。

**第二层 · 本地 CRDT 时钟退化**（实施第一层时发现，否则第一层无效）

原以为纳入 `counter` 就能防碰撞，实测发现 `bump_local_clock` 只从**传入**的时钟自增，而
`execute_and_record` 每次都构造 `vector_clock` 为空、`version` 为 0 的新 tuple。结果是：

```
第1次本地写入: ({'node-a': 1}, 1)
第2次本地写入: ({'node-a': 1}, 1)
第3次本地写入: ({'node-a': 1}, 1)
```

**每一条本地经验的 version 恒为 1、vc 计数恒为 1**。后果有二：同秒写入仍然撞主键（第一层
修复形同虚设）；`merge_crdt` 的版本裁决对本地写入完全失效——CRDT 在单节点场景下从未真正起作用。

修复：`VectorExperienceBank` 增加实例级单调序号 `local_seq`，写入时以它为准推进 vc 计数与
version。

**已知局限**：进程重启后 `local_seq` 归零。若重启前最后一条写入与重启后第一条落在同一秒
且计数相同，主键仍可能冲突。需要严格保证时应从存储层恢复最大计数。

**存量行**：`data/*.db` 中已写入的行保留旧格式主键，不会被新格式的检索命中，也不参与
未来的反熵摘要。项目处于 0.x，清库重建即可，未写迁移脚本。

回归保护：`crates/sisyphus-mem/tests/experience_id_collision.rs` 锁定同秒写入不丢与本地计数单调两项。

### P1 修复方案的取舍（已决策）

先评估过三个方案，**它们不等价**：

1. **主键纳入来源与计数** —— **已采纳**。行标识一旦编码来源与计数，紧凑摘要成立，
   ADR-010 才可能落地。
2. **提高时间戳精度**（毫秒 / 纳秒）—— 改动最小，但只是降低概率而非消除；且行标识
   仍不编码来源，本 ADR 的摘要前提依旧不成立。**未采纳。**
3. **内容判别式**（`exp:{ts}:{短哈希(context_text + action_id)}`）—— 同一目标同秒的
   两次执行会折叠为一行，而那是两次不同的执行，折叠等于丢掉其中一次的
   deviation_score，仍是数据损失；同样不解锁本 ADR。**未采纳。**

配套改动：把散落 6 处的 `format!("exp:{}", exp.timestamp)` 统一为 `experience_store_id`，
`record_experience` / `apply_remote_experience` / `insert_experience_full` 改为接收显式主键。

## 决策选项

### 选项 A：版本向量反熵（采纳）

连接建立后，由 **dial 方**（发起 `Join` 的一侧）把自己的版本向量发给对端；对端比对后，只回送 dial 方缺失的经验。

```
dialer ──Join──> peer
dialer ──SyncRequest{ vector_clock, offset }──> peer
peer   ──ExperienceSync[]──> dialer          （差量页，内容与实时广播同构）
peer   ──SyncPageEnd{ more, next_offset }──> dialer
dialer ──SyncRequest{ …, next_offset }──> peer   （仅当 more=true）
```

- 传输量正比于**真实差量**，与历史总量无关。
- 天然覆盖全部三个丢失窗口：首次连接 = 新节点加入；重连 = 断连补偿。
- 复用既有 `ExperienceSync` 载荷格式与 `merge_crdt` 判定，不引入第二套合并语义。

### 选项 B：连接后全量拉取（ADR-009 原案）

简单，但触发上面说的无界突发与 O(N²)。在经验库规模变大后不可接受。

### 选项 C：周期性重播

每个节点周期性重播最近 N 条经验。能缩短窗口但**永不闭合**，且浪费带宽。仅可作为 A 的补充（默认关闭），不作为唯一机制。

### 选项 D：外部注册中心（etcd / consul）

把经验写入中心存储，节点只做缓存。能力最强，但引入运维依赖，且与「经验库是节点本地数据」的现有设计冲突。明确排除。

## Decision

**采纳选项 A**，并把协议挂进既有的双向 `Stream`，不新增 RPC。

### 协议

在 `ClusterEvent` 的 oneof 中新增两个变体（复用已有流的收发方向）：

```proto
// ADR-010：dial 方发版本向量，请求对端补发差量。
SyncRequest sync_request = 5;

// ADR-010：peer 方发页尾标记，告知差量一页结束与翻页游标。
SyncPageEnd sync_page_end = 6;

message SyncRequest {
  // dial 方当前持有的 (node_id → 最大 counter)
  map<string, uint64> vector_clock;
  // 差量分页游标；首轮为 0
  uint64 offset = 2;
}

message SyncPageEnd {
  // 还有更多页时为 true
  bool more = 1;
  // 下一轮 SyncRequest 应携带的 offset
  uint64 next_offset = 2;
  // 本页实际回送条数（仅用于日志）
  uint32 delivered = 3;
}
```

差量本身以既有 `ExperienceSync`（`exp_key` / `version` / `vector_clock_json` / `payload`）发送，因此对端的处理路径与实时广播**完全相同**——`apply_remote` → `merge_crdt`，无需新增合并逻辑。

#### 为何是 `SyncPageEnd` 而不是 `SyncResponse`

原设计是 `SyncResponse{ experiences[], more }`。实施时改为「差量页发 `ExperienceSync`，页尾另发一个 `SyncPageEnd` 标记」，因为：

- 差量若包在 `SyncResponse` 里，接收端就需要**第二条**处理路径（解 `SyncResponse` 再逐条 apply），这与「不引入第二套合并语义」的初衷相背。发成裸 `ExperienceSync` 则完全复用广播路径。
- **页边界必须由服务端显式告知，不能由 dial 方本地计数推测。** 差量页与实时广播在 wire 上完全相同，dial 方无法分辨二者。若按「本页满 128 条即翻页」计数，期间到达的实时广播会被计入页内，使游标偏移错乱并漏发整页差量。`SyncPageEnd` 让页边界成为显式协议而非猜测。

### 触发点

- `run_stream` 在双向流建立后立即发起一次 `SyncRequest`（offset=0）。这一处同时覆盖「新节点加入」与「重连补齐」。
- 收到 `SyncPageEnd{ more=true }` 时带上 `next_offset` 继续请求，直到 `more=false`。
- 周期反熵（对应选项 C）暂不启用，留作配置项，等有真实丢失报告再评估。

### 分页与上界

- 单页最多 `SYNC_PAGE_SIZE = 128` 条，由 peer 侧强制，防止异常对端一次回灌无界数据。
- 分页是**无状态**的：每次请求都从本地库重新计算完整差量再取第 `offset` 段。库在翻页期间变化时，已发出的页不会作废；新写入的行可能落到已翻过的区间里，那些行由下一次重连的 `SyncRequest` 补齐。反之，若把差量缓存在会话里，新写入的行会因缓存快照过期而**永久漏发**。
- 游标按**差量集合的索引**推进，而非「本页实际发出一条数」：序列化失败的条目会被跳过，但游标若随之停下，下一轮会重算同一段而永远卡住。

### 失败语义

- `SyncRequest` 发送失败或翻页失败 → 记录 warn 并停止翻页；**不影响**实时广播路径，也不影响本地写入。
- 反熵整体是旁路：拿不到摘要就退化为只依赖实时广播，与反熵落地前的行为一致。

### 明确不在本 ADR 范围

- **删除传播**：当前是 LWW-Element-Set 且无 tombstone，删除无法跨节点传播。这是 ADR-009 已知的取舍，本 ADR 不改变它。
- **`StateProposal` 的消费方**：仍无消费者（TODO 阶段 9 后续），与本 ADR 无关。

## Consequences

### 正面

- 三个丢失窗口中前两个被闭合。
- 传输量与集群历史规模解耦。
- 不引入新依赖，不改变既有合并语义。

### 负面 / 待观察

- `vector_clock` 的体积随节点数增长。现实集群规模（个位数到几十个节点）下，map 很小；若将来节点数上千需要换 Bloom 过滤器或分片。
- 补齐期间对端要序列化并发送，可能与实时广播争抢带宽。在默认 128 条分页下影响有限。
- 「dial 方发起 pull」意味着被 dial 的一方不知道谁需要什么，也无法主动推送。取其简单与对称；若将来需要 push，再单独评估。
- **差量计算目前是全量的**：每次 `SyncRequest` 都要 `SELECT id FROM experiences` 取回**所有**主键，在内存里比对摘要。经验库达到十万行量级时，每次连接建立的开销会变得可观。当前的缓解是主键很短、比较是 O(N) 的 map 查找。若真的成为瓶颈，下一步是按 origin 分片存储 `counter`，用 `WHERE` 直接筛出差量而非全表扫描。
- **存量旧格式主键不参与反熵**（`exp:{timestamp}` 解析不出 origin/counter）。这是已记录的取舍，见「存量行」。

## 实施结果（2026-10-02）

已实施并验证，落在这些位置：

| 位置 | 内容 |
|------|------|
| `proto/cluster.proto` | `SyncRequest`、`SyncPageEnd`，挂进 `ClusterEvent` oneof（字段 5、6） |
| `sisyphus-types/src/experience.rs` | `ExperienceDigest`、`digest_from_keys`、`missing_keys`、`sorted_missing_keys` —— 纯函数，无 I/O |
| `sisyphus-mem/src/store.rs` | `experience_digest()`、`experience_keys()`、`load_experiences_by_keys()`（500 条分批，避开 SQLite 变量数上限） |
| `sisyphus-mem/src/experience_bank.rs` | `digest()` / `keys()` / `load_by_keys()` 访问接口 |
| `sisyphus-bridge/src/peer/anti_entropy.rs` | `build_sync_page`、`serve_sync_request`、`request_sync` 及摘要载荷序列化 |
| `sisyphus-bridge/src/peer/service.rs` | 收到 `SyncRequest` → 回送差量页 + `SyncPageEnd` |
| `sisyphus-bridge/src/peer/fallback.rs` | `PagingCursor` 翻页游标，只由 `SyncPageEnd` 推进 |
| `sisyphus-bridge/src/main.rs` | 装配：service 与拨号器都挂上真实经验库 |

### 实施中发现并修正的缺陷

1. **翻页从未闭环**（最严重）。`SyncResponse` 定义了 `more` / `next_offset`，但实现只发 `ExperienceSync` 从不发 `SyncResponse`，而 `fallback.rs` 累加了 `sync_offset` 却从不使用它发下一次请求。结果是差量超过 128 条时**只能补齐第一页**。已改为显式的 `SyncPageEnd` 协议。
2. **按条数推断页边界不可行**。中间版本尝试让 dial 方「本页满 128 条即翻页」。这在真实运行中是错的：实时广播会与差量混在同一个事件流里，被计入页内，使游标偏移错乱并漏发整页。已改为服务端显式告知页边界。
3. **11 元组超出 sqlx 上限**。反熵批量查询要一并取回 `id`，元组变成 11 元组，而 sqlx 只为 arity ≤ 9 的元组实现 `Type`。改为按列名取值（`experience_tuple_from_row`），保留既有的元组函数不动。
4. **`out_tx` 所有权**。反熵请求与广播共用 outbound 通道，但克隆必须发生在 `out_tx` 被 `move` 进广播 task **之前**，否则编译器拒绝借用。
5. **游标推进口径**。游标按差量集合索引推进，不是按本页实际发出一条数——否则遇到序列化失败的条目会永远卡在同一页。
6. **翻页区间会重叠（非「不重不漏」传输）**。这一点是变异测试揭露的，原实现与文档都误以为分页能做到不重不漏。每次 `request_sync` 都会重新导出摘要，但摘要粒度是 `(origin, max counter)` 的聚合值，不是逐行键集合。因此翻页期间请求方本地库已落库的行，仍可能因「origin 的最大 counter 未推进到足够高」而被算作差量而重发。实测：133 条差量在故意把游标强制回 0 时，末页发出 120 条而非应有的 5 条。不漏、不产生重复行（有保证）；传输不重叠（无保证）。

   这不是待修的缺陷，而是**有意的取舍**：重叠传输由 CRDT 幂等合并吸收，而消除重叠需要引入会话级去重状态，会在断连重连时丢失，反而更容易漏。ADR 已在测试注释与本节记录真实边界，避免后人误以为分页是 `exactly-once`。

### 验证

| 项目 | 结果 |
|------|------|
| `cargo check-all` | 通过 |
| `cargo check -p sisyphus-bridge --bins` | 通过（含 `main.rs` 装配） |
| `cargo clippy`（改动 crate） | 无新增警告 |
| `make ci-clippy`（全 workspace `-D warnings`） | 通过（修复了阻塞提交的工具链漂移，见下） |
| `make test-all` | **4715 通过 / 0 失败** / 12 忽略（基线 4705）。⚠️ `cargo test-all` 按项目约定**排除 `sisyphus-bridge`**，故此数字不含本 ADR 新增用例 |
| 反熵集成测试（`anti_entropy_integration`） | 7/7 通过（需单独运行） |
| 集群集成测试（`cluster_integration`） | 14/14 通过（需单独运行） |
| `sisyphus-types` 摘要纯函数用例 | 9/9 通过（已计入 `make test-all`） |

变异测试（手动注入缺陷，验证用例不是空断言）：

| 注入的缺陷 | 是否被捕获 |
|---|---|
| 服务端完全不应答 `SyncRequest` | ✅ 捕获（`test_dialer_pages` 超时失败） |
| `fallback.rs` 收到页尾也不翻页（**原实现缺陷**） | ✅ 捕获（实测只拿到 128 条，断言失败） |

第二行是本轮修复的核心回归锁定：注入后失败信息明确指出「只拿到 128 条」，与 ADR 描述的缺陷现象一致。

新增的 4 个 gRPC 全链路用例走**真实 `ClusterService` + 真实 `run_stream` 拨号器**，覆盖协议层测试看不到的部分：gRPC 编解码、流生命周期、翻页闭环。其中 `test_dialer_pages_through_delta_larger_than_one_page` 写入 133 条（非页大小整数倍），验证真实拨号器能把三页差量全部拉完——这是本轮修复的核心回归锁定。

新增的 11 个用例覆盖：空摘要全量拉取、重连差量、幂等、多 origin 隔离、**翻页循环覆盖全部差量**、**差量为单页整数倍时 `more` 必须为 false**、载荷可解码且重复应用被 CRDT 判为 `Equal`。

`crates/sisyphus-bridge/tests/anti_entropy_integration.rs` 走**真实 SQLite + 真实经验库**，专门覆盖纯函数测试看不到的部分：差量能否真的从库里按主键取回载荷、游标能否真的推进到下一页。其中「翻页循环覆盖全部差量」是 ADR-010 最关键的不变式——页边界丢失或游标不推进时该用例会失败。

注意该用例断言的是**覆盖完整**（不漏）而非**恰好一次**（不重）。在摘要不变的前提下分页恰好覆盖每条一次；摘要随本地落库推进后区间可能重叠（见上文缺陷 6）。用 `ExperienceDigest::new()` 构造固定摘要的纯函数用例走的正是前一种情形。

### 顺带修复：工具链漂移阻塞提交（非本 ADR 范围）

提交时被 pre-commit 的 `make ci-clippy` 卡住：`#[async_trait]` 会给 trait 的 async 方法加 `#[must_use]`，而其返回类型（boxed Future）本身已带 `#[must_use]`，**clippy 1.99.0** 新增的 `double_must_use` 将其判为冗余。这影响 8 个 crate 的全部 async trait（含 prost 生成的 service trait），与本 ADR 无关，是切到新工具链后暴露的既有问题。

试过在 `build.rs` 里用 `server_attribute` / `client_attribute` / `type_attribute` 压制，均无效——前两者只作用于生成的 client/server 结构体，`type_attribute` 对 service 无效。prost 的 `ServiceGenerator` 倒是可包装，但 `tonic_build` 的默认生成器是 `pub(crate)`，无法从外部替换。

最终采用 crate 级 `#![allow(clippy::double_must_use)]`，与仓库内既有的 `#![allow(clippy::result_large_err)]` 同一手法（`sisyphus-bridge-api`、`sisyphus-tools-llm`、`sisyphus-tools-mcp`、`sisyphus-bridge`、`sisyphus-core`、`sisyphus-intel`、`sisyphus-tools-agent`、`sisyphus-visual`）。放行范围限于本 crate，且只针对这一个 lint，不影响手写代码的其他约束。

### 遗留

- 第三个丢失窗口（订阅者 `Lagged`）仍未闭合：`ExperienceSyncHandler` 对 `RecvError::Lagged` 仍是 `continue`。这需要请求侧重放而非本 ADR 的差量机制，单独处理。
- 分页传输重叠（见上文缺陷 6）：如需消除，需把摘要粒度从 `(origin, max counter)` 改为逐行键集合，或引入会话级去重。当前规模下重叠传输的代价可接受。

## 实施路径

1. ✅ proto 新增 `SyncRequest` / `SyncPageEnd`，`proto_to_mgr_event` 增加对应分支。
2. ✅ `VectorExperienceBank` 暴露 `digest()` / `keys()` / `load_by_keys()`。
3. ✅ `ClusterServiceImpl` 收到 `SyncRequest` 后按 digest 计算差量并分页回送。
4. ✅ `run_stream` 在流建立后发起 `SyncRequest`，并按 `SyncPageEnd` 翻页。
5. ✅ 协议层集成测试（7 例）。✅ 双节点 gRPC 全链路用例（4 例，含 >128 条多页差量）。

## References

- ADR-009 § Risk 2（本 ADR 补充其未实施的缓解措施）与 § 2（CRDT 合并语义）
- `crates/sisyphus-bridge/src/peer/anti_entropy.rs` — 反熵核心
- `crates/sisyphus-bridge/src/peer/fallback.rs` — `run_stream`（触发点与翻页游标）
- `crates/sisyphus-bridge/src/peer/service.rs` — proto 转换与事件投递
- `crates/sisyphus-bridge/tests/anti_entropy_integration.rs` — 协议层集成测试
- `crates/sisyphus-bridge/tests/cluster_integration.rs` — 双节点 gRPC 全链路用例（真实拨号器翻页）
- `crates/sisyphus-mem/src/experience_bank.rs` — `apply_remote`
- `crates/sisyphus-types/src/experience.rs` — `merge_crdt` 与摘要纯函数
- TODO 阶段 10.2