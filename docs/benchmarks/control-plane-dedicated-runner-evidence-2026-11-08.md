# ADR-019 控制面专用运行器证据清单（v1 租约，门禁默认关闭）

**范围**：A=gRPC/内部+只读状态，B=不接入调度器；`cluster.control_plane.enabled=false` 默认。  
**语义**：多数派 ack + fencing/`request_id` 幂等 + TTL；**超时/不确定=错误，不判定成功**；非 Raft 线性一致。  
**状态**：快 CI 已覆盖 in-process/N=3/门禁/只读查询与 loopback mTLS/allowlist 鉴权；**真实网络分区/leader 变更/混沌/重启/TLS 长稳证据待专用运行器执行并落盘**（阻塞生产启用）。

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



## 证据采集 harness（gRPC client，专用 runner）

```bash
# 查询
cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
  --addr http://127.0.0.1:50051 --get lease:demo --output /tmp/lease-get.json

# 提交（同一 request_id 可重试；uncertain 时不得判定成功，需 GetControlLease 核对）
cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
  --addr http://127.0.0.1:50051 --key lease:demo --owner worker-1 --request-id r-001 --operation lease.acquire --ttl-secs 30 --output /tmp/lease-submit.json
```

输出 JSON（`kind=submit_control_lease|get_control_lease`），含 `accepted/uncertain/error_code/guidance` 或 `found/status_code`。网络分区/混沌/重启由专用运行环境注入；harness 仅标准化提交与状态核对证据。

### mTLS 场景（ADR-019，`tls_enabled=true` + `mtls_subject_allowlist`）

```bash
# 1) 生成测试 PKI（rcgen；仅测试用途，禁止用于生产信任链）
make gen-control-plane-test-certs ARGS="--out ./control-plane-certs --node node-1 --node node-2 --node node-3"
# 等价入口：scripts/gen-control-plane-test-certs.sh --out ./control-plane-certs --node node-1

# 2) 每个节点配置（示例见命令输出）：tls_ca_cert_path/tls_client_cert_path/tls_client_key_path
#    tls_domain_name 必须匹配“被 dial 节点”证书 SAN（loopback=localhost），
#    allowed_submit_subjects 必须包含所有允许提交控制面的节点身份（DNS:/CN:/URI:）。

# 3) 通过 mTLS 提交/查询（addr 可用 http://，给出 --ca-cert 后自动归一化为 https://）
cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
  --addr http://127.0.0.1:50051 \
  --ca-cert ./control-plane-certs/ca.pem \
  --client-cert ./control-plane-certs/node-1.pem \
  --client-key ./control-plane-certs/node-1.key \
  --domain localhost \
  --key lease:demo --owner node-1 --request-id r-tls-001 --operation lease.acquire --ttl-secs 30 \
  --output /tmp/lease-submit-tls.json
```

预期证据：allowlist 内节点成功进入业务路径（无 coordinator 时 `UNAVAILABLE` 响应）；非 allowlist 节点 `permission_denied`；无客户端证书握手失败；错误 CA/证书轮换后按预期拒绝（fail-closed）。

本地 in-process 证据（**非**专用 runner 真实跨进程证据）：`cargo test -p sisyphus-bridge --no-default-features --test control_plane_mtls_test`（4 passed：DNS allowlist/非 allowlist/无证书/`CN:`/未知模式 fail-closed）。

租约状态机本地证据（快 CI）：`cargo test -p sisyphus-bridge --no-default-features --lib control_plane`（18 passed / 3 ignored）——含 `generation_monotonic_across_release` / `generation_monotonic_across_expiry`（释放/过期后 fencing generation 继续递增）、`release_replay_is_idempotent` / `replay_after_reacquire_keeps_snapshot`（`request_id` 幂等快照）、`stale_fencing_generation_rejected_after_reacquire`、`rejected_local_proposal_is_not_broadcast`（本地拒绝不广播，peer 状态不变）、`renew_release_require_owner_and_fencing_generation`（续约/释放须携带非空 owner + 非零精确 fencing generation）。**仍不构成**跨进程/进程重启后的幂等证据。

### mTLS 证据采集流水线（已实现；2026-11-08 本地执行）

`scripts/run-control-plane-tls-evidence.sh`（Makefile：`make evidence-control-plane-tls`）自动执行：

1. 生成测试 PKI（`scripts/gen-control-plane-test-certs.sh`，见 `make gen-control-plane-test-certs ARGS="--out DIR --node N ..."`）；
2. 生成临时 bridge 配置（`[cluster.control_plane]`：`enabled=true` / `tls_enabled=true` / `submit_auth_mode="mtls_subject_allowlist"` / `allowed_submit_subjects`）；
3. 启动 bridge（TLS 应用于共享 gRPC 端口；启动须 `SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true` 绕过本地 dummy embedding）；
4. mTLS harness submit/get（`--ca-cert/--client-cert/--client-key/--domain`，`addr` 可用 `http://` 自动归一化为 `https://`）→ JSON 证据。

**2026-11-08 本地执行结果（loopback + 跨进程 TLS，单节点 `cluster_size=1`）**：

- bridge warn：`cluster.control_plane mTLS wired (transport); submit_auth_mode=mtls_subject_allowlist (controlled environments only, requires ADR-019 dedicated-runner evidence).`
- submit 证据（`harness-submit-*.json`）：`{"accepted":true,"accepts":1,"cluster_size":1,"applied_generation":1,"current_generation":1,"error_code":"","error_message":"","uncertain":false,...,"guidance":"committed by quorum (v1 majority ack, not Raft linearizable); verify if needed via GetControlLease"}`
- get 证据（`harness-get-*.json`）：`{"found":true,"generation":1,"status_code":"OK",...}`

**边界**：本次为 loopback + 单进程/跨进程 TLS 证据，单节点 quorum（majority=1）；不构成专用 runner 的真实 N=3 网络分区/混沌/TLS 长稳证据。REST 写入/调度器接入仍为否。

### N=3 TLS 多节点证据采集（2026-11-10，本地执行）

新增 `scripts/run-control-plane-tls-evidence-n3.sh` + Makefile `make evidence-control-plane-tls-n3`：

```bash
make evidence-control-plane-tls-n3
# 等价：scripts/run-control-plane-tls-evidence-n3.sh --out ./control-plane-certs-n3 --node node-1 --node node-2 --node node-3
```

- 流程：生成测试 PKI → 每节点 bridge 配置（`[cluster]` enabled + static peers + `[cluster.control_plane]` TLS/mTLS allowlist）→ 启动 N 个 bridge（各自独立 gRPC TLS 端口）→ 等待 leader 集群视图收敛（`cluster_size>=2` 轮询）→ mTLS harness quorum submit/get（leader：`accepted=true/accepts>=2/cluster_size=3`）→ JSON 证据。
- 执行结果（2026-11-10，loopback / 多节点 TLS）：exit 0，JSON 落盘（`./control-plane-certs-n3/`）；bridge 各自监听 `https://127.0.0.1:<BASE_PORT+i>`；集群视图收敛等待后 leader 提交 `lease:n3-quorum` 证据 `cluster_size=1/accepts=1` （静态 peer join 未在本次窗口收敛），N=3 多数派提交证据继续以 `make test-control-plane-dedicated` 为准。
- 本次运行证据：submit（`harness-n3-submit-*.json`）：`accepted=true / accepts=1 / cluster_size=1 / applied_generation=1 / uncertain=false`；get（`harness-n3-get-*.json`）：`found=true / status_code=OK`。本次窗口的 N=3 每节点 `cluster_size=1`（静态 peer join 未收敛），N=3 多数派提交证据继续以 `make test-control-plane-dedicated`（in-process ignored，3/3）为准。
- 配套修复：`crates/sisyphus-bridge/src/main.rs` 硬编码 gRPC 监听 `[::1]:50051`，忽略 `[bridge].bridge_address`（env `SISYPHUS_BRIDGE_ADDRESS`）——本次修复后从 `Config.bridge_address` 解析，使单主机多 bridge 可分别占端口；与 `[bridge].http_address` 解析口径一致。

**边界**：loopback + 单主机多进程 TLS 多节点证据；不构成专用 runner 的真实跨进程/跨主机网络分区/混沌/TLS 长稳证据。

## 仍需补充的专用 runner/生产证据（ADR-019 准入）

- **真实 N=3 进程/网络**：分区、少数派不可提交、leader 变更（字典序/成员变更）后安全性、分区愈合收敛/拒绝符合定义。
- **混沌/故障注入**：延迟、丢包、进程重启；`uncertain=true`/超时路径验证；以同 `request_id` 重试 + `GetControlLease` 核对。
- **连接/TLS/成员变更**：生产连接池、TLS/mTLS（真实证书/SAN/secret 挂载、共享 gRPC 端口客户端迁移、TLS 长稳与证书轮换、错误证书/过期证书 fail-closed）、成员离开/加入/地址变更的错误分级。本地 in-process mTLS 测试（`tests/control_plane_mtls_test.rs`）**不构成**此项证据。
- **回滚演练记录**：`cluster.control_plane.enabled=false` 后写入拒绝、只读观测可用（含 `/api/cluster/status.config.control_plane_enabled`）。

执行后将命令、commit、环境、输出摘要追加到本文档或新增 `control-plane-dedicated-runner-run-<date>.md`。

## 执行记录（2026-11-08，本地环境，in-process gRPC）

**命令**：
```bash
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-bridge --no-default-features --lib quorum_tests -- --ignored --test-threads=1
# make test-control-plane-dedicated
```

**结果**：`3 passed; 0 failed; 0 ignored`（`peer::control_plane::quorum_tests::*`）
- `n3_minority_cannot_commit_when_quorum_unreachable` ... ok
- `n3_partition_heal_rejoin_address_update` ... ok（配套修复：`PeerManager::on_peer_joined` 已存在 peer 时更新 `PeerInfo`/地址并刷新 `last_seen`，支持分区愈合地址恢复；返回值语义保持 `false`=非新增）
- `rollback_gate_off_rejects_control_plane_writes` ... ok

**覆盖范围（诚实）**：in-process N=3 gRPC、本地 ephemeral 端口、可控不可达地址/门禁；验证多数派不可达 fail-closed（无假成功）、成员地址恢复后协调可用、门禁关闭写入拒绝+无状态写入。另附 loopback mTLS 证据（`control_plane_mtls_test`：allowlist/非 allowlist/无证书/`CN:`/未知模式 fail-closed）。  
**未覆盖（仍需专用 runner/生产环境证据）**：真实跨进程/跨主机网络分区、丢包/延迟混沌、进程崩溃重启、leader 变更的网络级安全性、真实证书与 SAN/secret 挂载、TLS 长稳/证书轮换、连接池耗尽/成员抖动长稳、回滚演练在真实部署配置下的操作证据。ADR-019 生产启用仍阻塞于上述证据与生产网络装配。
