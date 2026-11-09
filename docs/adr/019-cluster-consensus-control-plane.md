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


## 实现状态（2026-11-08，v1 默认方案：分布式任务租约；A=gRPC/内部+只读状态，B=不接入调度器）

- 已完成：`ClusterConsensus::propose` 无 proposer 时返回明确错误（消除虚假成功）；`[cluster.control_plane].enabled`（默认 `false`，env/example/book/`/api/cluster/status.config.control_plane_enabled`）；`cluster.proto` `ProposeControl`（peer 本地 apply）+ `SubmitControlLease`（协调节点多数派提交，返回 `ControlLeaseResponse{accepts,cluster_size,uncertain}`）；`peer::control_plane::{ControlPlaneState,ControlPlaneCoordinator,LeaseQuorumOutcome}`（租约 `lease.acquire/renew/release`，`request_id` 幂等、TTL、fencing generation；协调节点**本地有界决策缓存**仅缓存确定性结果，不缓存 `uncertain`；**多数派 ack + 不确定/超时=错误不判定成功**，非 Raft 线性一致）；`ClusterServiceImpl::{propose_control,submit_control_lease}` + `with_control_plane_coordinator`（DISABLED/UNAVAILABLE 门禁）；`consensus::{GrpcPeerProposer,control_proposal_from_value}`；`ClusterRuntime`/`main.rs`/`MySisyphusBridge` 门禁装配并复用同一 state/coordinator（内部 accessor）；`PeerManager::list_peers()` 地址解析（补 `http://`，排除 self）。
- 验证（快 CI）：`--lib control_plane` 18 passed / 3 ignored（含 acquire/renew/release/fencing、持有拒绝、幂等、`rejected_local_proposal_is_not_broadcast`、`generation_monotonic_across_release`、`generation_monotonic_across_expiry`、`release_replay_is_idempotent`、`replay_after_reacquire_keeps_snapshot`、`stale_fencing_generation_rejected_after_reacquire`、`renew_release_require_owner_and_fencing_generation`、`n3_quorum_acquire_succeeds_with_majority`、`n3_quorum_fails_when_majority_unreachable`、`n3_request_id_idempotent_across_coordinator`、`n3_renew_release_and_fencing_via_coordinator`、`n3_submit_control_lease_via_grpc_quorum`、`get_control_lease_read_only_and_gate`、`coordinator_follows_membership_changes_fail_closed`、`uncertain_submission_guidance_retry_and_get`）；`--lib consensus` 12 passed；`--lib peer::tls` 4 passed（TLS fail-closed + 空 allowlist/缺失证书拒绝）；`--test control_plane_mtls_test` 4 passed（见下）。专用运行器矩阵与执行/证据清单见 `docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md`（`make test-control-plane-dedicated`；`#[ignore]` 3 项在 2026-11-08 本地 in-process 环境 3/3 通过，**非**真实跨进程网络分区/混沌/重启/TLS 证据）。配套修复：`PeerManager::on_peer_joined` 支持已存在 peer 地址/元数据更新（分区愈合）。
- fencing/幂等加固（2026-11-08，v1 缺陷修复）：释放/过期不再删除 key 条目，`record.generation` 作为高水位持续递增——此前释放或过期后重新获取会退回 generation=1，使旧 fencing token 重新有效；`lease.release` 成功后条目保留为 tombstone，同 `request_id` 重试返回同一结果（此前返回 `LEASE_NOT_FOUND`）；重放返回请求当时的租约快照而非当前租约（此前会把新 owner 冒充成旧请求结果）；同时移除生产路径 `expect("exists")`；协调节点本地拒绝的提案不再广播（被拒请求不得修改 peer 状态）；续约/释放还要求携带非空 owner 与非零精确 fencing `expected_generation`（空 owner / `expected_generation=0` 不再豁免校验，避免任意allowlist 节点释放或续约他人租约）。
- TLS/mTLS 与 `SubmitControlLease` 鉴权（2026-11-08 追加，按建议实施；**代码接线，非生产准入**）：
  - `peer/tls.rs::load_control_plane_tls` 加载 PEM CA + 节点身份（cert/key）+ `tls_domain_name`；`tls_enabled=true` 但路径为空/不可读/不完整 => `wired=false` 且 **fail-closed 拒绝装配**。`tls_enabled+wired` 时 TLS 应用于**共享 gRPC 端口**（server）与 coordinator/`GrpcPeerProposer`（client mTLS）；主 bridge gRPC 客户端也须迁移 TLS，部署需统一规划（必要时再评估拆分端口）。
  - `peer/tls.rs::verify_peer_subject_allowlist`（`x509-parser`）：提取 peer 证书 Subject CN / SAN DNS/URI，匹配 `allowed_submit_subjects` 的 `DNS:`/`URI:`/`CN:`/裸值；空 allowlist、无证书、不可解析、不匹配 => `false`（fail-closed）。
  - `ClusterServiceImpl`：`ProposeControl`/`SubmitControlLease` 在 `mtls_subject_allowlist` 下校验 `TlsConnectInfo.peer_certs()`，缺 TLS peer 信息/无证书/不匹配 => `permission_denied`；`none` 放行；**未知模式 fail-closed**（绕过 `main.rs` 装配检查也拒绝）。装配仅允许 `tls_enabled && wired && allowed_submit_subjects 非空`。
  - rustls provider：依赖图可能同时启用 `ring`/`aws-lc-rs`，显式安装 `ring` provider（bridge `main`/harness/loader），避免 TLS 握手 panic。
  - 本地 in-process 证据：`crates/sisyphus-bridge/tests/control_plane_mtls_test.rs` 4 项——DNS allowlist 命中（`UNAVAILABLE` 响应而非 RPC 错误）、非 allowlist 主体 `permission_denied`（`SubmitControlLease` + `ProposeControl`）、无客户端证书握手/RPC 失败、`CN:` 命中、未知模式 fail-closed。**这是 loopback/单进程证据，不替代专用 runner 的真实跨进程 TLS/网络/混沌/重启证据。**
  - 运维工具：`make gen-control-plane-test-certs ARGS="--out DIR --node node-1 ..."`（`examples/gen_control_plane_test_certs.rs`，rcgen，仅测试用途；`scripts/gen-control-plane-test-certs.sh` 为入口）；`control_plane_harness --ca-cert/--client-cert/--client-key/--domain`。
- 未完成（阻塞生产启用）：**不确定提交客户端指引**（超时后不得盲判成功，需以同 `request_id` 重试并结合状态查询；跨节点/进程重启后 idempotency 仅由各节点 state 保障）；`GrpcPeerProposer` 生产连接池/成员变更深度集成；**真实证书签发/SAN 与 `tls_domain_name`/peer 身份对齐、secret 挂载、共享端口 TLS 客户端迁移**；专用运行器执行证据（真实 N=3 分区/少数派不可提交/leader 变更安全/分区愈合/混沌延迟丢包重启/**TLS 长稳与证书轮换/错误证书 fail-closed**）与回滚演练记录；REST 写入+OpenAPI/webui（若 A 扩展）；调度器租约接入（B=不接入）。**默认 `cluster.control_plane.enabled=false`，未满足准入不得生产启用。**
