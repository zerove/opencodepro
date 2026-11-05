# ADR-015: 反向反熵拉取——服务端在流建立后主动拉取 dialer

**Status**: Implemented（2026-11-05）
**Deciders**: core team
**Related**: ADR-009（分布式 v2 架构）, ADR-010（反熵补偿）, ADR-011（Lagged 补齐）, ADR-012（分页游标）, ADR-013（补齐信号复用）

## 背景

ADR-010 定下的反熵协议是 **「dial 方发起 pull」**：拨号方建连后发
`SyncRequest{vector_clock}`，被 dial 方据此算差量分页回送。

这一设计让两个丢失窗口（流建立前广播丢失、断连期间写入回流）在
「dialer 重连 → dialer 再拉」这条路径上闭合。但它把**发起方**钉死在了 dialer
上，把**被动方**永远排除在主动补偿之外。

在单侧拨号拓扑下（A dial B，B 不 dial A）这条边界直接暴露成永久性数据丢失：

- A 断连重连后会主动向 B 拉齐，B 的断连期间写入因此能回来；
- 但 B 在断连期间写入的本地经验，只可能被 A 的拉取带回——而 B 永远不会去
  拉 A。全连接生产拓扑下两侧都 dial，因此不受影响；单侧拓扑下「被拨方」的写入
  没有回流路径，重连重连后依然只剩实时广播（无连接窗口内无路可发）。

`docs/optimization-plan-2026-11-05.md` §5.6 曾把这记为未闭环项：单侧拨号拓扑
的收敛边界。

## 决策

**服务端（被 dial 方）在每条双向流建立后，也主动发起一次 `SyncRequest`，
由 dialer 侧应答差量；服务端按 `SyncPageEnd{more, next_cursor}` 分页驱动。**

要点：

1. **复用既有协议，不新增 RPC。** `SyncRequest` / `SyncPageEnd` /
   `ExperienceSync` 在双向 `Stream` 上的语义是「发起方 → 应答方」的通用关系：
   发起方的摘要 + 游标决定应答方的差量。服务端把自身摘要当成发起方的身份即可，
   无需改 proto、无需引入 push 模型。

2. **双向流本身已是双向的**，服务端把主动请求塞进既有 outbound channel
   （`out_tx_sync`），由 dialer 经同一 inbound 流回应——零协议形状变化。

3. **服务端分页状态机独立维护。** 服务端在流建立后发空游标的初始拉取，
   之后每收到一页 `SyncPageEnd` 就用 `next_cursor` 发下一页，直到 `more=false`。
   拨号方自己的拉取（建连初始 + `Lagged` 补齐）沿用原有 `PagingCursor`
   （ADR-012 键集游标）状态机，互不干扰。

4. **应答与拉取的职责边界保持清晰：** 服务端「应答 dialer 的拉取」由
   `handle_sync_request` 完成；服务端「拉 dialer」由 stream 任务里的 outbound
   状态机驱动。两者走同一条 `SyncRequest → pages + SyncPageEnd` 链路，
   不存在第二套合并语义。

5. **失败语义不变：** 拉取发送失败只记日志、清游标；不影响实时广播与本地写入。

6. **可观测性新增：** `ClusterServiceImpl` 带
   `outbound_sync_requests` 计数（初始拉取 + 翻页续拉），经 `JoinGateHandle::
   outbound_sync_requests_seen()` 暴露给测试。

## 后果

### 正面

- 单侧拨号拓扑下，被拨方断连期间的写入在重连后的反向拉取中被带回，
  分区 → 恢复后的最终一致性从「dialer 单方面拉」扩展到「双方都可能拉」。
- 全连接生产拓扑的行为不变：双方在建连后都会发一次拉取，但此时差量通常
  为空（已同步），传输量仍与真实差量成正比（与 ADR-010 的设计目标一致）。

### 负面 / 待观察

- 每个新流建立后都多了一跳双向交互（双方各发一次拉取、可能各需要分页），
  在「已同步集群」里这两跳都回空页，成本可忽略；在「严重偏离同步」的集群里
  成本等于真实差量，符合 ADR-010 的取舍。
- 中途「服务端 outbound 广播 `Lagged`」仍未被主动补偿（周期性重放是 ADR-010
  选项 C，未启用）。本次只闭合了「断连 → 重连」这一缺口；
  若未来需要「无连接窗口之外」的主动补偿，仍以周期性重放为路径。

## 实施结果（2026-11-05）

实现位置：

| 位置 | 内容 |
|------|------|
| `crates/sisyphus-bridge/src/peer/service.rs` | 流建立后主动发初始 `SyncRequest`；`outbound_pull_cursor` 分页状态机；`sync_page_end_of` 单一判定点；`handle_sync_request` 改为 `pub(crate)` |
| `crates/sisyphus-bridge/src/peer/fallback.rs` | dialer 侧 `run_stream` inbound 处理新增 `SyncRequest → handle_sync_request` 应答分支（此前只应答自己的拉取） |
| `crates/sisyphus-bridge/src/peer/service.rs` | 新增 `outbound_sync_requests` 计数与 `JoinGateHandle::outbound_sync_requests_seen()` |

### 实施中发现的问题

- 拨号侧原本把「应答对端拉取」写在 `if let Some(cursor) = paging.as_mut()` 分支
  内，语义上把「自己的分页会话」和「应答」混在一个状态机里。ADR-015 后明确：
  应答不推进本侧分页游标，只写 outbound；本侧分页只由本侧自己发起的拉取
  （`paging` / `outbound_pull_cursor`）的 `SyncPageEnd` 推进。

## 验证

### 门禁 / 测试

| 项目 | 结果 |
|------|------|
| `cargo fmt --all --check` | ✅ 无 diff |
| `cargo clippy -p sisyphus-bridge --no-default-features --all-targets -- -D warnings` | ✅ 0 warning |
| `cargo test -p sisyphus-bridge --no-default-features --test chaos_one_sided_reverse_pull` | ✅ 1 passed（新增） |
| `cargo test -p sisyphus-bridge --no-default-features --test chaos_partition_convergence` | ✅ 1 passed（全连接回归） |
| `cargo test -p sisyphus-bridge --no-default-features --test anti_entropy_integration` | ✅ 12 passed |
| `cargo test -p sisyphus-bridge --no-default-features --test anti_entropy_partition_convergence` | ✅ 1 passed |

### 变异验证（反向拉取必须真被测到）

1. 临时把服务端流建立后的初始 `SyncRequest` 发送逻辑置为不发送
   （`if false { … }` 包裹）；
2. `cargo test -p sisyphus-bridge --no-default-features --test chaos_one_sided_reverse_pull`
   → **稳定失败**：`首连未完成（B 主动拉了 0 次 …）`；
3. 还原源码后该用例 **✅ 通过**（全绿）。

结论：新增的单侧拓扑用例不是「看起来通过」的断言，它把「服务端反向拉取」
这条路径本身锁死。

## 遗留

- 周期性重放（ADR-010 选项 C）未启用；反向拉取只在「流建立」时触发。
- 服务端 outbound 广播 `Lagged` 的主动补偿仍走重连兜底；本次未扩大范围。
- 旧版本（无反向拉取能力）服务端与新版本 dialer / 新版本服务端与旧版本 dialer
  的互操作性：新服务端对旧 dialer 发请求，旧 dialer 会丢弃（行为退化为旧语义），
  属于可接受的版本偏差，与 ADR-012「老方读 `offset = 0` 即从头重来」的幂等设计
  同源。
