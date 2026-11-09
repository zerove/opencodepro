# 架构决策记录（ADR）索引

本目录记录 Sisyphus 的重要架构决策。每份 ADR 保留**当时的推理与取舍**，
后续变更以「Amended / Superseded」形式追加，不重写历史。

| # | 标题 | 状态 | 日期 |
|---|------|------|------|
| [0005](./0005-cognitive-loop-v2.md) | Cognitive Loop v2 — 自适应深度、分支评估与异常检测 | Amended（ParallelBranchPredictor 已移除） | 2026-08-26 修订 |
| [002](./002-phase-3b-agent-team-architecture.md) | Phase 3b 多代理团队架构 | Proposed | 2026-06-25 |
| [003](./003-modular-restructure.md) | 单体到模块化重构 | Accepted | 2026-07-01 |
| [004](./004-workspace-isolation.md) | 多租户工作区隔离 | Accepted | — |
| [005](./005-sandboxd-daemon-architecture.md) | Sandboxd 守护进程架构 | Accepted | 2026-07-30 |
| [006](./006-configuration-system.md) | 配置系统 | Accepted | 2026-07-30 |
| [007](./007-production-grade-standards.md) | 最终产品开发标准 | Accepted | 2026-08-28 |
| [008](./008-shared-http-client.md) | 生产级共享 HTTP 客户端 | Accepted | 2026-08-28 |
| [009](./009-distributed-v2-architecture.md) | 分布式 v2 架构（peer 协议 + Experience CRDT + 共识） | Implemented（含 2026-10-02 更正） | 2026-09-05 |
| [010](./010-experience-anti-entropy.md) | 跨节点经验的反熵补偿（重连补齐） | Implemented | 2026-10-02 |
| [011](./011-lagged-catchup-and-digest-holes.md) | 广播 Lagged 的闭合与摘要空洞表达 | Implemented | 2026-10-02 |
| [012](./012-keyset-pagination-cursor.md) | 反熵分页改用主键游标 | Accepted | 2026-10-02 |
| [013](./013-reuse-catchup-receiver-across-reconnect.md) | 补齐信号跨重连复用接收器 | Implemented（修正记录） | 2026-10-02 |
| [014](./014-break-core-tools-dependency.md) | 解除 sisyphus-core 对 sisyphus-tools-* 的反向依赖 | Accepted（四阶段完成） | — |
| [015](./015-reverse-anti-entropy-pull.md) | 反向反熵拉取（服务端流建立后主动拉 dialer） | Implemented（2026-11-05） | 2026-11-05 |
| [016](./016-federated-experience-aggregation.md) | 联邦经验聚合（入站经验质量门槛） | Implemented（2026-11-05） | 2026-11-05 |
| [017](./017-strategy-weights-consensus.md) | 集群策略权重同步（每节点快照 + generation 取新） | Implemented（2026-11-06） | 2026-11-06 |
| [020](./020-cluster-control-plane-rest-scheduler-integration.md) | Cluster control plane — REST 写入 / scheduler lease integration | Accepted（REST 写入=否 / 调度器 B=不接入） | 2026-11-10 |

## 编号说明

`0005` 与 `005` 是两份不同决策，编号在历史上发生冲突（`0005` 为四位补零写法）。
为避免已发布链接失效，**不重命名**；新 ADR 从 `015` 起按三位编号。

## 反熵链路阅读顺序

理解跨节点经验同步，按以下顺序读：

1. **ADR-009** — peer 协议与 Experience CRDT 的数据模型。
2. **ADR-010** — 为什么需要反熵（三个丢失窗口）与摘要协议。
3. **ADR-011** — 摘要必须能表达空洞（`Lagged` 打破「连续前缀」前提）。
4. **ADR-012** — 分页必须用主键游标（差量会随接收缩短，下标会漂移）。
5. **ADR-013** — 补齐信号必须复用接收器（否则信号根本没进拨号器）。
6. **ADR-015** — 服务端在流建立后主动拉 dialer（反向反熵；闭合单侧拨号拓扑的回流缺口）。

对应实现与测试：

- 协议/差量：`crates/sisyphus-bridge/src/peer/anti_entropy.rs`
- 接收与应用：`crates/sisyphus-bridge/src/peer/experience_sync.rs`
- 摘要与空洞：`crates/sisyphus-types/src/experience.rs`
- 端到端：`crates/sisyphus-bridge/tests/anti_entropy_integration.rs`、
  `anti_entropy_partition_convergence.rs`、`cluster_integration.rs`、
  `chaos_partition_convergence.rs`、`chaos_one_sided_reverse_pull.rs`

## 联邦经验聚合阅读顺序

理解「入站经验要不要收」，读：

1. **ADR-010 / 011 / 013 / 015** — 先保证「一条都不丢」（差量、空洞、补齐、
   反向拉取）。
2. **ADR-016** — 再决定「哪些不该收」（质量门槛，默认关闭）。

实现：`crates/sisyphus-types/src/experience.rs`（评分纯函数）、
`crates/sisyphus-bridge/src/peer/experience_sync.rs`（门槛接入）、
`crates/sisyphus-types/src/config/sub.rs`（`[cluster.federated]`）。

## 集群策略权重阅读顺序

理解「集群里哪个策略真的有效」，读：

1. **ADR-009** — 集群事件总线（`StateProposal` 是既有的双向消息类型）。
2. **ADR-016** — 先有「单条经验要不要收」，才有「跨节点策略统计」的需求。
3. **ADR-017** — 每节点一份快照 + `generation` 取新；为什么不用
   `Consensus::propose`（生产无 proposer，只写日志）。

实现：`crates/sisyphus-types/src/strategy_weights.rs`（合并/聚合纯函数）、
`crates/sisyphus-bridge/src/peer/strategy_weights.rs`（周期发布 + 入站合并）、
`crates/sisyphus-bridge/src/peer/service.rs`（入站 `StateProposal` 投递）、
`GET /api/strategy/weights`（`crates/sisyphus-bridge/src/http/handlers.rs`）。
端到端：`crates/sisyphus-bridge/tests/cluster_integration.rs`
（`test_two_node_strategy_weights_propagate_and_merge`）。
