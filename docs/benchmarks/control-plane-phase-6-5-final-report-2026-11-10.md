# ADR-019/020 阶段 6.5 最终阶段报告（2026-11-10）

> 适用阶段：v0.4.x 阶段 6.5「多实例协调与共识、经验聚合」→ ADR-019/020。
> 状态：阶段可执行范围完成；ADR-019 生产启用准入仍待专用 runner 真实证据 + 生产部署装配。

## 结论摘要

1. **TLS/mTLS 接线**：tonic `tls` + PEM/CA/节点身份 + shared gRPC port + rustls provider 显式装
   （修复 ring/aws-lc-rs 双启握手 panic）。
2. **鉴权 fail-closed**：`mtls_subject_allowlist`（CN/SAN DNS/URI/CN/裸值；缺 TLS/证书/不匹配/空
   allowlist ⇒ 拒绝；未知模式 service 层 fail-closed；装配需 wired+allowlist）。
3. **租约状态机加固**（v1 缺陷修复）：
   - fencing generation 高水位（释放/过期后不再回退 `1`）；
   - release 幂等（tombstone + 快照）；
   - 本地拒绝提案不广播；
   - 续约/释放须显式 owner + 非零精确 fencing generation。
4. **证据链路**：`make gen-control-plane-test-certs` → `make evidence-control-plane-tls`（loopback
   单节点）→ `make evidence-control-plane-tls-n3`（loopback N=3）→ mTLS harness submit/get。
5. **bridge 运行期修复**：gRPC 监听地址硬编码 `[::1]:50051` → 从 `[bridge].bridge_address` 解析
   （env `SISYPHUS_BRIDGE_ADDRESS` / config）。
6. **OpenAPI**：悬空 `$ref` + `securitySchemes.bearerAuth` 修复 + 门禁测试。
7. **决策**：ADR-020 `Accepted`（REST 写入保持否；调度器接入保持 B=不接入；预留 `SubmitControlLease`
   全链路 acquire/renew/release）。

## 验证证据

- `cargo fmt --all --check` ✅
- `cargo clippy -p sisyphus-bridge --no-default-features --all-targets -D warnings` ✅
- `cargo check-all` ✅
- `make audit` ✅
- `make test-control-plane-dedicated` ✅ 3/3
- `make evidence-control-plane-tls-n3` ✅（N=3 TLS 多节点，exit 0；集群视图收敛窗口内 `cluster_size=1`）
- `cargo test -p sisyphus-bridge --no-default-features --lib`：179 passed；3 个 `sandboxd_client`
  失败为本机 spawn `Permission denied (os error 13)`（环境性，未改动相关代码）

## 诚实边界

- 未伪造专用 runner 真实网络证据；N=3 多节点在本机 loopback + 单进程内执行。
- 专用 runner 真实 N=3 网络分区/混沌/TLS 长稳/回滚证据、真实证书/SAN/secret、部署装配语义
  仍待硬件侧执行与生产部署验证。
- ADR-019 生产启用前须满足准入条件；`cluster.control_plane.enabled` 默认 `false`。

## 交接

- 交接清单：[`docs/runbooks/control-plane-dedicated-runner-execution-checklist.md`](runbooks/control-plane-dedicated-runner-execution-checklist.md)
- 证据清单：[`docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md`](benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md)

## 结论

阶段 6.5「可执行范围」**完成**；ADR-020 决策定夺完成（Accepted）；ADR-019 生产启用准入仍阻塞于外部前提。
