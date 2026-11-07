# ADR-019: ClusterConsensus 控制面（强一致决策）准入条件

**Status**: Proposed（2026-11-08，**条件触发**；未在生产路径启用）  
**Deciders**: core team  
**Related**: ADR-009（分布式 v2）、ADR-017（StateProposal 数据面）、ADR-018（协调/聚合范围，6.5）

## 背景

ADR-017/018 已确立：集群内**策略/经验类同步**走 `StateProposal`/`ExperienceSync` +
`generation`/CRDT/反熵的**最终一致数据面**，并由 `/api/cluster/status` 等只读接口观测。
`sisyphus-core` 定义的 `Consensus` trait（`is_leader`/`get_leader`/`propose`）在 bridge 中
有实现装配，但生产装配历史上存在 `proposer: None` 的路径（`propose()` 仅记录日志），且
`PeerManager` 的 leader 为字典序启发式视图——**不构成强一致提交语义**。

当出现需要“提交成功/失败”或互斥的**控制面**操作（全局配置变更、互斥任务/租约分配、独占
迁移）时，直接复用当前数据面会把冲突消解交给 LWW/幂等规则，可能违反业务互斥。此类需求
**必须**先满足本 ADR 的准入条件，再实现。

## 决策

**`ClusterConsensus::propose/quorum` 仅用于强一致控制面；在未满足准入条件前，生产路径
禁止启用可提交的 proposer。**

### 1. 适用场景（允许）

- 全局配置变更的原子提交（要么全体按版本接受，要么拒绝）。
- 互斥任务/租约/leader 租约分配（同一资源同一时刻至多一个持有者）。
- 需要线性一致/多数派确认语义的控制面操作（由具体子 ADR 明确一致性级别）。

### 2. 不适用场景（禁止复用强一致替代）

- 经验元组同步、策略权重快照、可丢弃/可合并的观测数据（继续使用现有数据面）。
- 用 `propose()` “看起来同步了”但不检查提交结果的路径。

### 3. 准入条件（实现前必须全部满足）

1. **语义定义**：单独子 ADR/设计文档定义操作模型（key 空间、值编码、版本/epoch、读写语义、
   与 `StateProposal` 的分工——控制面 vs 数据面，禁止双写双裁决）。
2. **Proposer 装配**：生产 `main`/`bridge_service` 明确 proposer 实现、生命周期、超时、重试上限；
   `proposer: None` 路径对控制面操作必须返回明确错误（不可静默成功/仅日志）。
3. **故障模型**：定义分区、leader 变更、节点重启、重复提交（幂等键）、提交超时/不确定结果的
   客户端处理；明确多数派/quorum 计算与成员变更边界。
4. **可观测**：`/api/cluster/status` 或专用只读端点暴露控制面状态（role/term/epoch/last_commit
   等，按实现而定）；不暴露敏感载荷。
5. **验证矩阵（最低）**：
   - 单元：propose 成功/拒绝/超时/重复提交幂等。
   - 集成：N=3 正常提交；少数派不可提交；leader 变更后安全性；分区愈合后收敛/拒绝符合定义。
   - 混沌/故障注入：延迟、丢包、进程重启（复用现有 chaos 框架模式）。
   - 回归：`cluster/anti_entropy/strategy/experience_sync` 现有套件保持绿；OpenAPI/book/CHANGELOG 同步。
6. **回滚**：控制面开关可关闭并回退到“拒绝控制面写入、只读观测可用”的安全状态。

### 4. 非目标

- 在本 ADR 中选定具体共识算法实现（Raft/Paxos/etcd/内置）——由满足准入条件的后续实现 ADR 决定。
- 改变现有经验/策略最终一致数据面的合并语义。
- 跨集群控制面。

## 结果与影响

- 为 TODO 阶段 6「多实例协调与共识」的强一致部分提供明确门禁，避免与 ADR-018 冲突。
- 在无强一致需求时，系统继续以已验证的数据面运行；`6.5` 保持“条件触发”。

## 验收（触发实现时）

- 满足第 3 节全部准入条件，附证据（测试/ADR/配置/文档）。
- 生产路径默认安全（未显式启用控制面时，不产生虚假提交成功）。


## 实现状态（2026-11-08，v1 默认方案：分布式任务租约）

- 已完成：`ClusterConsensus::propose` 无 proposer 时返回明确错误（消除虚假成功）；`[cluster.control_plane].enabled`（默认 `false`，env/example/book/`/api/cluster/status.config.control_plane_enabled`）；`cluster.proto` `ProposeControl` RPC；`peer::control_plane::ControlPlaneState`（`lease.acquire/renew/release`，`request_id` 幂等、TTL、fencing generation）；`ClusterServiceImpl::propose_control` 门禁/UNAVAILABLE/DISABLED 拒绝；`consensus::GrpcPeerProposer`；`main.rs` 门禁装配；单元测试（`control_plane` 3、`consensus` 12 含 Mock N=3/禁用门禁 gRPC 测试）、`cluster_status_rest_test` 更新。
- 未完成（阻塞生产启用）：gRPC `PeerProposer` 与 `PeerManager` 地址解析/成员变更/连接池的生产装配；租约操作经 `ClusterConsensus` quorum 的端到端多节点提交路径；N=3 真实网络分区、少数派不可提交、leader 变更安全、重复提交幂等（跨节点）、超时不确定语义、混沌/重启矩阵；回滚演练；REST 控制面写入（如需）+ OpenAPI/webui 契约。
