# ADR-018: 多实例协调与共识、经验聚合范围（v0.4.x 收口）

**Status**: Proposed（2026-11-08，范围收敛；待按子阶段实施）  
**Deciders**: core team  
**Related**: ADR-009（分布式 v2 / 集群事件总线）、ADR-010（反熵）、ADR-011（Lagged 补齐）、ADR-015（反向反熵拉取）、ADR-016（联邦经验聚合 Phase A）、ADR-017（策略权重同步 Phase B–G）

## 背景

TODO 阶段 6 遗留两项模糊表述：「多实例协调与共识」「经验聚合服务」。在 v0.4.0
完成 ADR-016/017（经验 CRDT 同步 + 策略权重快照/generation/来源校验）及 Phase 6.1/6.2b
可观测（`/api/cluster/status`、经验同步/StateProposal 计数）之后，继续沿「模糊项直接编码」
推进会产生两类风险：

1. **重复造轮子**：把已存在的 `ExperienceSync`（CRDT）+ 反熵 + 经验查询接口，重新包装成
   一个与现有数据面语义不一致的“聚合服务”。
2. **共识范围漂移**：在 `ClusterConsensus` 生产装配未定义 proposer/故障语义的前提下引入
   `propose/quorum`，与已打通的 `StateProposal` 事件路径形成双轨，分区/重放/回环语义难以
   同时保证。

本 ADR 的目标是：**把 TODO 模糊项收敛为可执行、可测试、可回滚的子阶段边界**，优先复用
已验证的数据面与可观测面。

## 决策

### 1. 经验聚合 = 复用现有数据面 + 查询/观测视图；默认不新建独立聚合进程

**默认架构（v0.4.x）**：

| 能力 | 现有实现（复用） | 说明 |
|---|---|---|
| 经验写入/传播 | `PeerBroadcastExperienceBank` → `PeerManager.broadcast_experience_sync` → 对端流；`ExperienceSyncHandler` 入站 → `VectorExperienceBank::apply_remote` | CRDT/LWW+vector clock，入站通道不回流 |
| 缺失补偿 | ADR-010/011/015 反熵、Lagged catchup、反向拉取 | 幂等合并，收敛不依赖“恰好一次” |
| 入站质量门 | ADR-016 `FederatedAggregatorConfig` + `evaluate_experience_quality`（默认可不启用/按配置） | 不改变合并规则，只影响是否接收 |
| 本地查询 | `/api/experience/similar`、`VectorExperienceBank`/经验库接口 | “聚合查询”首先在本地已收敛视图上完成 |
| 可观测 | `/api/cluster/status` 的 `peer_manager.experience_sync`、`state_proposal_broadcasts` | 只读，不改变数据路径 |

**“经验聚合服务”在 v0.4.x 的含义**：桥接内的**查询/观测视图层**（复用已收敛经验），而不是
单独部署的服务进程、不是新的共识写路径、不是替代 CRDT 的中心化合并器。

**触发新建独立服务/进程的条件（满足任一再立 ADR）**：
- 需要**跨多个集群**汇聚（当前 peer/反熵边界是单集群）。
- 需要**强一致全局查询 SLA**（当前是最终一致 + 本地查询）。
- 需要独立于 bridge 的扩缩容、租户隔离或审计存储边界，且无法通过现有 store/API 扩展满足。
- 单 bridge 节点的经验库/向量检索成为已量化瓶颈（附基准与 profile），且拆分优于本地优化。

### 2. 多实例协调 = `StateProposal` 事件数据面为主；`ClusterConsensus` 仅用于强一致决策（后置）

**已采用的数据面（继续复用）**：
- **策略/状态类同步**：`StateProposal` + `generation` 单调/幂等（ADR-017）、入站投递 `deliver_*` 防回环、来源标注与一致性校验（Amendment 4）。
- **领导者视图（弱语义）**：`PeerManager` 的字典序 leader / `ClusterConsensus` trait 的只读视图用于观测与启发式；**不**作为互斥/强一致提交的唯一依据，除非后续 ADR 明确其故障模型。

**`ClusterConsensus::propose/quorum` 的引入边界**（需单独 ADR + 测试矩阵后才可实现）：
- 适用场景：全局配置变更提交、互斥任务/租约分配、需要“提交成功/失败”语义的控制面操作。
- 前置条件：生产装配明确 proposer 实现与生命周期；定义分区、leader 变更、重复提交、超时与重试语义；补齐故障注入/混沌测试；与 `StateProposal` 明确分工（控制面 vs 数据面），避免双写双裁决。
- **当前不做**：在 main/bridge 装配中启用未定义语义的 proposer；用 `propose()` 日志成功冒充提交成功。

### 3. 可观测与运维优先于新语义

后续协调/聚合工作默认先补齐：`/api/cluster/status` 与指标（已有 broadcast/deliver/applied/rejected、strategy proposal 计数）、结构化日志、故障场景复现。任何改变收敛语义的改动必须附带：
- 变异/反例验证（至少 1–2 处）；
- 现有 cluster/anti_entropy/chaos/strategy/experience_sync 回归；
- OpenAPI + `book/src/rest-api.md` + `CHANGELOG.md [Unreleased]`。

## 非目标（本阶段明确不做）

- **跨节点策略协商**（ADR-017 Phase E 遗留，决策语义未定义）。
- **定向转发/拆通道**（ADR-017 Phase G 已推迟；触发条件：N≥16 或载荷显著变大并有量化证据）。
- **策略权重全量快照 `/v2` 迁移**（待载荷/兼容性需求明确）。
- **独立“经验聚合服务”进程**（除非满足上文触发条件并另立 ADR）。
- **在未定义 proposer/故障语义时启用强一致 `propose/quorum` 生产路径**。

## 建议实施顺序（后续子阶段）

1. **6.3（观测收口）**：按需补充 `ExperienceSync`/反熵失败原因分类计数（仍只读），并在 book 中补充排障 runbook；不改合并。
2. **6.4（范围内增强）**：仅在量化证据支持下做小步优化（如反熵/Lagged 参数、查询视图索引）；保持 ADR-016/017 兼容。
3. **6.5（强一致控制面，条件触发）**：若出现互斥/提交需求，先写 `ADR-019 ClusterConsensus control plane`，再实现；禁止跳过。

## 验收标准

- 对“经验聚合服务”与“多实例协调/共识”的实现争议，可直接引用本 ADR 判定：**复用现有数据面优先；独立服务/强一致需触发条件+新 ADR**。
- 相关 PR/提交在描述中标注对应子阶段（6.3/6.4/6.5）与证据。

## 结果与影响

- 减少与 ADR-010/011/015/016/017 重叠的重复实现。
- 将高风险共识变更隔离到显式决策点，保护现有最终一致数据面。
- TODO 阶段 6 的剩余项可拆分为“文档/观测收口”和“条件触发的控制面 ADR”，避免长期悬空。
