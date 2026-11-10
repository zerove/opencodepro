# ADR-019 控制面专用 runner 证据执行 — 交接清单（Handoff Checklist）

> 适用阶段：6.5「多实例协调与共识、经验聚合」→ ADR-019 准入（真实证据执行）。
> 关联：[`docs/runbooks/control-plane-dedicated-runner-tls-evidence.md`](./control-plane-dedicated-runner-tls-evidence.md)（执行步骤）、
> [`docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md`](../../benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md)（证据汇总）。
> 边界：本清单只固化交接顺序；所有证据结论必须来自真实运行，禁止虚构。

## 交接前置

- [ ] 专用 runner（N=3 节点，真实网络可达，loopback 或独立网段）已准备；
- [ ] 时钟同步（chrony）；端口规划（gRPC `50051+` / REST `8080` 等）；
- [ ] 无残留 `sisyphus-bridge` / `sisyphus-sandboxd` 进程。

## 执行清单

1. **PKI（专用 runner，真实 CA，仅测试用途）**
   - [ ] `make gen-control-plane-test-certs ARGS="--out <DIR> --node n1 --node n2 --node n3"`
   - [ ] 私钥权限 `0600`；CA/节点证书双用途（ServerAuth+ClientAuth，SAN 含 `DNS:<n>,DNS:localhost,IP:127.0.0.1`）。
2. **节点配置（每节点 TOML）**：`[cluster]`（enabled + peers）+ `[cluster.control_plane]`（tls_enabled / tls_ca_cert_path / tls_client_cert_path+key / tls_domain_name / submit_auth_mode / allowed_submit_subjects）。
3. **启动 3 bridge**：`SISYPHUS_CONFIG=<cfg> SISYPHUS_BRIDGE_ADDRESS=http://<host-or-ip>:50051 SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true ./target/debug/sisyphus-bridge`（独立端口；TLS 由 control_plane 装配生效）。
4. **集群收敛 + 多数派提交**：leader 等待 `cluster_size>=2` → quorum submit/get（leader：`accepted=true / accepts>=2 / cluster_size=3`）→ get 核对。
5. **真实网络证据矩阵**：
   - [ ] N=3 正常提交（多数派 ack）；
   - [ ] 少数派不可提交（分区注入）→ `QUORUM_NOT_REACHED`/`uncertain`，无假成功；
   - [ ] leader 变更后安全性；分区愈合/重入；
   - [ ] 混沌延迟/丢包；kill-restart；超时/不确定 → 同 `request_id` 重试 + Get；
   - [ ] TLS 长稳/证书轮换/错误证书 fail-closed；回滚演练（`enabled=false` → 写入 `DISABLED`）。
6. **落盘**：harness JSON + commit/环境/拓扑摘要 → `docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md`。

## 验收

- [ ] 全部真实证据采集完成且与 in-process 结果一致；
- [ ] ADR-019 准入条件满足后，方可评估 `cluster.control_plane.enabled` 生产启用（默认仍为 `false`）。
