# ADR-019 控制面专用 runner TLS 真实证据 Runbook（生产准入前置）

> 适用阶段：6.5「多实例协调与共识、经验聚合」→ ADR-019 准入第 3/5/6 项
> （真实证书/SAN、真实网络证据、TLS 长稳/回滚）。
> 边界：本 runbook 指导在**专用 runner + 真实网络**（N=3 节点，loopback 或独立网段）执行
> TLS/mTLS 控制面证据采集；`scripts/run-control-plane-tls-evidence.sh`（`make
> evidence-control-plane-tls`）是 **loopback 冒烟/单节点流水线**，本 runbook 用于其
> 升级为专用 runner 真实多节点证据。

## 前置条件

1. 专用 runner（N=3 节点，真实网络可达）已就绪，且：
   - `openssl`/`rcgen` PKI 工具可用（推荐 `make gen-control-plane-test-certs ARGS="--out <DIR> --node <n1> --node <n2> --node <n3>"`）；
   - `cargo build -p sisyphus-bridge --no-default-features` 完成；
   - bridge 可监听独立端口（`SISYPHUS_BRIDGE_ADDRESS=<host>:50051`）。

## 步骤 1：生成 PKI（仅测试用途，禁止用于生产信任链）

```bash
make gen-control-plane-test-certs ARGS="--out ./control-plane-certs --node n1 --node n2 --node n3"
# 等价：scripts/gen-control-plane-test-certs.sh --out ./control-plane-certs --node n1 --node n2 --node n3
```

产物：`ca.pem/ca.key`、`<n>.pem/<n>.key`（`<n>` 为节点 DNS 名；SAN 含 `DNS:<n>,DNS:localhost,IP:127.0.0.1`）。

## 步骤 2：为 N 节点生成 bridge 证据配置

每个节点一份 TOML（示例以 n1/n2/n3）：

```toml
[cluster]
enabled = true
node_id = "n1"            # 各节点唯一
cluster_name = "sisyphus-control-plane-evidence"

[cluster.control_plane]
enabled = true
tls_enabled = true
tls_ca_cert_path = "<DIR>/ca.pem"
tls_client_cert_path = "<DIR>/n1.pem"   # 本节点自身身份（TLS server identity + client mTLS identity）
tls_client_key_path = "<DIR>/n1.key"
tls_domain_name = "n1.internal"         # 必须匹配本节点被对端 dial 的 SAN
submit_auth_mode = "mtls_subject_allowlist"
allowed_submit_subjects = ["DNS:n1", "DNS:n2", "DNS:n3"]
```

- `tls_domain_name` 必须与**被 dial 节点**的 SAN 一致（节点间互连用 `DNS:<n>`；loopback
  冒烟用 `localhost`）。
- `allowed_submit_subjects` 必须覆盖所有允许提交控制面的节点身份。

## 步骤 3：启动 N 个 bridge（TLS 共享 gRPC 端口语义）

- 每个节点主机起一个 bridge：`SISYPHUS_CONFIG=<cfg> SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true ./target/debug/sisyphus-bridge`
- TLS 应用于该 bridge 的 gRPC server（共享端口内的 server TLS）；`tls_enabled+wired` 时
  coordinator / `GrpcPeerProposer` 用 client mTLS 互连。
- 等待就绪：loopback 用 `nc -z`（IPv4/IPv6）轮询；真实网段用连接探测。

## 步骤 4：TLS 证据采集（harness mTLS）

```bash
for n in n1 n2 n3; do
  # 对每个节点做 SubmitControlLease + GetControlLease（同 request_id 幂等重试）
  ./target/debug/control_plane_harness --addr https://<n-host>:50051 \
      --ca-cert <DIR>/ca.pem --client-cert <DIR>/$n.pem --client-key <DIR>/$n.key \
      --domain <n>.internal \
      --key lease:<n>-<ts> --owner <n> --operation lease.acquire --ttl-secs 30 \
      --output /tmp/evidence-submit-<n>.json
  ./target/debug/control_plane_harness --addr https://<n-host>:50051 \
      --ca-cert <DIR>/ca.pem --client-cert <DIR>/$n.pem --client-key <DIR>/$n.key \
      --domain <n>.internal --get lease:<n>-<ts> --output /tmp/evidence-get-<n>.json
done
```

## 步骤 5：真实网络证据矩阵（专用 runner 注入分区/混沌/重启）

按 ADR-019 验证矩阵执行并把输出汇总到
`docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md`：

- N=3 正常提交（多数派 ack；单节点 `cluster_size=1` 为 `majority=1`）。
- 少数派不可提交（分区注入）→ `accepted=false` + `uncertain` 或 `QUORUM_NOT_REACHED`；**无假成功**。
- leader 变更后安全性；分区愈合/重入。
- 混沌延迟/丢包；kill-restart；超时/不确定 → 同 `request_id` 重试 + `GetControlLease` 核对。
- TLS 长稳：证书轮换 / 错误证书 / CA 丢失 → fail-closed 拒绝；恢复后重新验证。
- 回滚：[`cluster.control_plane`] `enabled=false` → 写入 `DISABLED`，只读观测可用。

## 步骤 6：落盘

把真实运行的 harness JSON、commit、环境、拓扑摘要追加到
`docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md`，并满足 ADR-019 准入后
方可评估 `cluster.control_plane.enabled` 生产启用（默认仍为 `false`）。

> 本 runbook 只做证据执行指导；所有结论必须来自真实运行，禁止基于测试 PKI 之外的证书/场景
> 伪造证据。
