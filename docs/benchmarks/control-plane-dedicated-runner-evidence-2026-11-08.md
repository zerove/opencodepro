# ADR-019 控制面专用运行器证据清单（v1 租约，门禁默认关闭）

**范围**：A=gRPC/内部+只读状态，B=不接入调度器；`cluster.control_plane.enabled=false` 默认。  
**语义**：多数派 ack + fencing/`request_id` 幂等 + TTL；**超时/不确定=错误，不判定成功**；非 Raft 线性一致。  
**状态**：快 CI 已覆盖 in-process/N=3/门禁/只读查询；**真实网络分区/leader 变更/混沌/重启证据待专用运行器执行并落盘**（阻塞生产启用）。

## 快 CI（已通过）

```bash
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-bridge --no-default-features --lib control_plane
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-bridge --no-default-features --lib consensus
cargo check-all && make audit
```

证据函数（`crates/sisyphus-bridge/src/peer/control_plane.rs`）：`ControlPlaneState::{acquire_renew_release_and_fencing,acquire_rejected_when_held,request_id_idempotent_replay}`；`quorum_tests::{n3_quorum_acquire_succeeds_with_majority,n3_quorum_fails_when_majority_unreachable,n3_request_id_idempotent_across_coordinator,n3_renew_release_and_fencing_via_coordinator,n3_submit_control_lease_via_grpc_quorum,get_control_lease_read_only_and_gate,coordinator_follows_membership_changes_fail_closed,uncertain_submission_guidance_retry_and_get}`。  
共识门禁（`crates/sisyphus-bridge/src/consensus.rs`）：`test_propose_rejected_as_leader_without_proposer`、`test_grpc_peer_proposer_propose_control_disabled_gate`、`test_grpc_peer_proposer_quorum_n3_mock`、`bench_consensus_5_nodes`（基线见 `consensus-control-plane-baseline-2026-11-08.md`）。

## 专用运行器矩阵（`#[ignore]`，需 `--ignored`/`--include-ignored` + 稳定本地网络端口/时序）

```bash
make test-bridge-dedicated
# 或定向：
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-bridge --no-default-features --lib n3_partition_heal_rejoin_address_update -- --ignored --nocapture
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-bridge --no-default-features --lib rollback_gate_off_rejects_control_plane_writes -- --ignored --nocapture
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-bridge --no-default-features --lib n3_minority_cannot_commit_when_quorum_unreachable -- --ignored --nocapture
```

| 矩阵 | 测试 | 预期证据 | 状态 |
|------|------|----------|------|
| 分区愈合/成员地址恢复 | `control_plane.rs::quorum_tests::n3_partition_heal_rejoin_address_update` | 不可达成员恢复后 coordinator 使用更新成员视图；多数派可提交或显式不确定（不假成功） | `#[ignore]`，待专用 runner 执行记录 |
| 回滚演练 | `control_plane.rs::quorum_tests::rollback_gate_off_rejects_control_plane_writes` | gate off => `SubmitControlLease`/写路径 `DISABLED`，无状态写入；只读/状态路径行为按 ADR | `#[ignore]`，待专用 runner 执行记录 |
| 少数派不可提交 | `control_plane.rs::quorum_tests::n3_minority_cannot_commit_when_quorum_unreachable` | 多数派不可达时 `accepted=false` 且 `uncertain` 或多数派不足；无假成功 | `#[ignore]`，待专用 runner 执行记录 |

## 仍需补充的专用 runner/生产证据（ADR-019 准入）

- **真实 N=3 进程/网络**：分区、少数派不可提交、leader 变更（字典序/成员变更）后安全性、分区愈合收敛/拒绝符合定义。
- **混沌/故障注入**：延迟、丢包、进程重启；`uncertain=true`/超时路径验证；以同 `request_id` 重试 + `GetControlLease` 核对。
- **连接/TLS/成员变更**：生产连接池、TLS/mTLS（如需）、成员离开/加入/地址变更的错误分级。
- **回滚演练记录**：`cluster.control_plane.enabled=false` 后写入拒绝、只读观测可用（含 `/api/cluster/status.config.control_plane_enabled`）。

执行后将命令、commit、环境、输出摘要追加到本文档或新增 `control-plane-dedicated-runner-run-<date>.md`。
