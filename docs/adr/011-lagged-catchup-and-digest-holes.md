# ADR-011: 广播 Lagged 的闭合，与反熵摘要的空洞表达

**Status**: Implemented (2026-10-02)
**Deciders**: core team
**Supersedes**: ADR-010 的摘要语义（`(origin, max counter)` → `(origin, {max, holes})`）
**Related**: ADR-010 反熵补偿、ADR-009 § 2 Experience CRDT

## Context

ADR-010 落地后，经验同步的三个丢失窗口中前两个已闭合。第三个——订阅者落后——当时被记为遗留：

> 第三个丢失窗口（订阅者 `Lagged`）仍未闭合：`ExperienceSyncHandler` 对 `RecvError::Lagged` 仍是 `continue`。这需要请求侧重放而非本 ADR 的差量机制，单独处理。

当时的判断是「触发一次已有的差量拉取即可」。实施中发现这个判断对了一半，而另一半比预期严重得多。

## 结论摘要

1. **触发侧**：补齐信号 + 从 offset 0 重新拉取，是正确且必要的一层。实现它需要修复两处真实缺陷。
2. **协议侧**：光有触发侧**完全不够**。ADR-010 的摘要 `(origin → max counter)` 在协议层就无法表达空洞，因此对端算出「差量为空」，一个缺失行都不会补。必须同时升级摘要。

第二点是本 ADR 的实质发现。第一点做对了也修不好它。

## ADR-010 摘要对空洞结构性失明

ADR-010 的摘要把「我有什么」压缩成「每个 origin 我持有到第几条」，差量判定是 `counter > local[origin]`。这隐含一个前提：

> 单个 origin 内持有的行构成 `1..=max` 的**连续前缀**。

前提成立时它是正确且高效的。但广播 `Lagged` 恰好打破它：接收端丢的是**中间**若干条，尾部照常抵达，`max` 依旧很大。于是对端看到「对方已有 1..=70」，算出差量为空，那个洞永久留在库里。

关键在于这不是触发条件的问题，而是表达能力的上限。无论补齐信号发多少次、时机多准，对端每次收到的都是同一份「我什么都全了」——**重放一千次也不会有一行被补上**。

实测（`catchup_signal_reruns_anti_entropy`，两节点真实 gRPC）：期望 70 条，B 停在 51 条，缺失的恰好是 counter 43、44、46、47、48… 而 45、49、50 都在。教科书式的空洞。

### 前置验证：counter 确实连续

修复前先确认了紧凑表达成立的前提（`record_with_id` 里 `local_seq.fetch_add(1) + 1`，重启后由 `max_local_counter` 恢复）：

- 单个 origin 内写入序号从 1 起**严格连续**，不跳号。
- 因此「空洞」是一个可枚举的有限集合，且正常节点恒为空。

这让 `holes: Vec<u64>` 成为恰当的表达：正常传输量不变（空洞为空），只有真正丢失时才变长——而那正是需要修复的量。ADR-010 「传输量正比于真实差量」的结论**依然成立**，不需要推翻。

## 变更

### 协议层：摘要携带空洞

`sisyphus-types`：

```rust
pub struct OriginDigest { pub max: u64, pub holes: Vec<u64> }
pub type ExperienceDigest = BTreeMap<String, OriginDigest>;

impl OriginDigest {
    pub fn holds(&self, counter: u64) -> bool {
        counter > 0 && counter <= self.max && self.holes.binary_search(&counter).is_err()
    }
}
```

`missing_keys` 从「比较上界」改为「逐条断言是否确实持有」。语义从*推测*变为*事实*：持有集合是对本地行的直接观察，不再是从 `max` 推导出的结论。

`digest_from_keys` 必须先按 origin 收集实际持有的 counter 再折叠。直接增量折叠成 `max` 会在折叠那一步就永久丢掉空洞信息——这是实现时最容易踩空的地方。

proto 侧新增 `OriginDigest` 消息，`SyncRequest.origins` 为 `repeated`。旧字段 `vector_clock` 保留并标记 `deprecated`（字段号不回收）。老节点只发 `vector_clock`，服务端把 `origins` 为空按「请求全量」处理，行为天然退化为正确而非错误。

### 触发侧：补齐信号

`ExperienceSyncHandler` 增加可选 `catchup_tx`。收到 `Lagged(n)` 后广播补齐信号，拨号侧 `run_stream` 在 `select!` 中接收并从 offset 0 重新发起反熵。

实现过程中修复了三处真实缺陷，都不是「加个信号」那么简单：

1. **`select!` 复用已完成的 future**。最初把接收写成一个常驻 `async` 块，`recv()` 一旦返回该 future 就已完成，在 `select!` 中复用会立即反复 ready，形成无休止的反熵请求风暴。改为每次 `select!` 分支重建 future（`recv_catchup`）。顺带让 `Closed` 自然退化为永久 pending，不再反复唤醒 `select!`。

2. **两个无 session ID 的分页会话交错**。已有分页在途时若直接再发 offset 0，两个会话的 `SyncPageEnd` 会互相污染游标，把偏移推到错误位置。用 `catchup_pending` 标志挂起，等当前会话收尾再从 0 重来——不接着当前偏移，因为挂起期间本地可能又新增了经验，只补差量会漏。

   （补记：这条延后分支的测试一开始是**假覆盖**——用例断言通过，但变异（把挂起改成丢弃）也能存活。实测（注入 tracing subscriber 抓日志）的原因是两个分支一次都没执行：洪泛是同步写入的，21 个信号比拨号器建连早约 11ms 就发完了，而拨号器用 `broadcast::Receiver::resubscribe` 取接收器，看不到订阅之前的信号。**发送成功不等于信号被消费**。已在 ADR-012 修正用例、装上分支命中证据并加变异捕获；接收器跨重连复用的修复见 ADR-013。）

3. **时间节流压制了真正的丢失**（最隐蔽）。最初加了 1 秒节流防请求风暴。它的推理是「一簇 Lagged 的首次触发能覆盖整簇的丢失」。这个假定是错的：补齐请求在**发出时**快照摘要，而丢失发生在快照**之后**的事件同样需要补齐。实测中整轮洪泛在毫秒级完成，1 秒窗口把 18000 次 Lagged 压成一次请求，而那次快照远早于写入结束——后续 30 条经验再无人请求。已移除。

   节流并非必需：拨号侧的 `catchup_pending` 已保证同一时刻只有一个分页会话，密集信号被合并成一次延迟补齐。上界由同步往返耗时决定，而非人为时间窗。盲目加节流掩盖了一个更基础的协议缺陷。

### 诊断过程值得记录

变异测试推翻了我最初的诊断。第一轮我认定根因是节流窗口，移除节流后端到端仍然失败（70 期望 / 57 实际）。继续追查才发现摘要失明——**两个缺陷互相掩盖**：摘要在看不见洞时，无论发多少次补齐请求都不会收敛；而节流在摘要修好后依然压制着后续信号。只有两个都修，测试才转绿。

我第一版的端到端用例本身也是假通过的：洪泛只有 40 条，而入站通道容量是 256，根本没触发 `Lagged`；末尾的经验走正常广播就凑齐了数。加了一条守卫断言——「补齐信号必须真的触发过」——它立刻把这个假通过暴露出来。真实制造 `Lagged` 靠的是：`deliver_inbound_experience_sync` 是同步调用，循环内无 await 点，handler 任务在这段里根本不会被调度，因此单批超过容量就**确定性地**溢出，不依赖竞态。

## 验证

`sisyphus-types` 单元测试 75/75；`sisyphus-bridge` 集群集成 17/17（含真实 gRPC 端到端）；`anti_entropy_integration` 7/7 通过。

注意 `anti_entropy_integration` 用的是内存存储（`VectorMemoryStore` + `DummyEmbeddingClient`），不是 SQLite。它覆盖反熵的请求/应答/分页/幂等合并路径，但**不**覆盖 SQLite 的持久化与崩溃恢复。持久化恢复路径的验证不在本 ADR 范围内——本次改动只触及摘要的计算与传输，不触及落盘。

新增用例：
- `hole_inside_contiguous_prefix_is_still_detected` — 摘要必须报出尾部内的洞
- `digest_records_holes_within_contiguous_prefix` / `sparse_origin_reports_every_absent_counter` — 空洞采集与 `holds` 语义
- `lagged_broadcast_triggers_catchup_signal` — `Lagged` 确实发出信号
- `no_catchup_signal_without_subscription` — 反向对照，未订阅时不发信号
- `catchup_signal_reruns_anti_entropy` — 真实丢失后逐条集合比对收敛

变异测试：

| 注入的缺陷 | 结果 |
|---|---|
| `missing_keys` 退回「只看 max」（ADR-010 原语义） | ✅ 单元与端到端均捕获，缺失键恰为空洞 |
| `Lagged` 只记日志不发信号（ADR-011 前行为） | ✅ 捕获 2 例，反向对照仍通过 |
| 间隙扫描漏报前导空洞（`prev` 初值错误） | ✅ `sparse_origin_reports_every_absent_counter` 捕获 |

第一行是关键：变异复现出的缺失键形态（43、44、46、47…，而 45、49 在）与修复前的实测完全一致，说明测试锁定的是真实缺陷而非表象。

写测试时我加的一条断言「连续持有时不应产生空洞」还纠正了既有测试数据的不真实：`exp:200:node-b:7` 在旧摘要下无害，但在空洞语义下意味着 1..6 真实缺失。生产中写入序号不跳号，该形态只出现在测试数据里，但语义必须成立——把它谎报成「持有 1..=7」只会让真正缺失的行永远补不上。

## 遗留

- **旧格式主键仍无法补齐**：`exp:{timestamp}`（ADR-010 之前）解析不出 origin/counter，既不参与摘要也不会被请求。这是已记录的取舍（项目 0.x，清库重建），本 ADR 未改变。
- **摘要本身是快照**：补齐请求发出后新增的写入需等下一次 `Lagged` 或重连才被拉取。正常广播承担这部分，无额外延迟保证。
- **分页传输重叠**：如需消除，需引入会话级去重。当前规模下代价可接受。（游标本身的缺陷已在 ADR-012 修正。）
- **补齐信号会被发送失败吞掉**：`resume_pending_catchup` 原本在发请求之前清位。已在 ADR-012 修正。

## References

- ADR-010 § 遗留（第三窗口）与 § 缺陷 6
- `crates/sisyphus-types/src/experience.rs` — `OriginDigest` / `missing_keys` / `digest_from_keys`
- `crates/sisyphus-bridge/src/peer/anti_entropy.rs` — 协议层差量计算与 wire 转换
- `crates/sisyphus-bridge/src/peer/experience_sync.rs` — `Lagged` → 补齐信号
- `crates/sisyphus-bridge/src/peer/fallback.rs` — `run_stream`、`recv_catchup`、`catchup_pending`
- `crates/sisyphus-mem/src/experience_bank.rs` — `record_with_id` 的 counter 分配（连续性前提）
- `crates/sisyphus-bridge-api/proto/cluster.proto` — `OriginDigest`
