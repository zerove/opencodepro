# ADR-010: 跨节点经验的反熵补偿（重连补齐）

**Status**: Proposed (2026-10-02) — 协议与触发点已定，待实施
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
dialer ──SyncRequest{ vector_clock }──> peer
peer   ──SyncResponse{ experiences[], more }*──> dialer
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
message SyncRequest {
  // dial 方当前持有的 (node_id → 最大 version)
  map<string, uint64> vector_clock;
}

message SyncResponse {
  repeated ExperienceSync experiences = 1;
  // 还有更多页时为 true，对端继续发送直到 false
  bool more = 2;
}
```

`SyncResponse` 复用既有 `ExperienceSync`（`exp_key` / `version` / `vector_clock_json` / `payload`），因此对端的处理路径与实时广播**完全相同**——`apply_remote` → `merge_crdt`，无需新增合并逻辑。

### 触发点

- `run_dial_loop` 在 `connect_and_join` 成功后立即发起一次 `SyncRequest`。这一处同时覆盖「新节点加入」与「重连补齐」。
- 周期反熵（对应选项 C）暂不启用，留作配置项，等有真实丢失报告再评估。

### 分页与上界

- `SyncResponse` 单次最多 `sync_page_size` 条（默认 128，可配置），`more=true` 时继续。
- 上限由 peer 侧强制，防止恶意/异常对端一次回灌无界数据。

### 失败语义

- `SyncRequest` 超时或失败 → 记录 warn，退回指数退避重连；**不影响**实时广播路径，也不影响本地写入。
- 对端返回 `Equal` 为主（说明已同步）→ 正常结束，不视为错误。

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

## 实施路径

1. proto 新增 `SyncRequest` / `SyncResponse`，`proto_to_mgr_event` / `mgr_event_to_proto` 增加对应分支。
2. `PeerManager` 暴露 `request_sync()`：订阅本节点版本向量并发出 `SyncRequest`。
3. `ClusterServiceImpl` 收到 `SyncRequest` 后，按 digest 计算差量并分页回送。
4. `run_dial_loop` 在 join 成功后调用 `request_sync()`。
5. 测试：
   - 双节点：A 断连期间写入，B 重连后 A 的经验到达 B（覆盖丢失窗口 2）。
   - 新节点加入后能拉到既有经验（覆盖丢失窗口 1）。
   - 重复触发 `SyncRequest` 不产生重复经验（幂等）。

第 5 项的前两条可直接复用 `crates/sisyphus-bridge/tests/cluster_integration.rs` 已有的双节点夹具（`start_sync_node` / `wait_for_link`）。

## References

- ADR-009 § Risk 2（本 ADR 补充其未实施的缓解措施）与 § 2（CRDT 合并语义）
- `crates/sisyphus-bridge/src/peer/fallback.rs` — `connect_and_join` / `run_dial_loop`（触发点）
- `crates/sisyphus-bridge/src/peer/service.rs` — proto 转换与事件投递
- `crates/sisyphus-mem/src/experience_bank.rs` — `apply_remote`
- `crates/sisyphus-types/src/experience.rs` — `merge_crdt`
- TODO 阶段 10.2