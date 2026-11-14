# 已知问题（已修复）：弱模型下的 JSON 保真度与门禁退化

**状态**：✅ 已修复（2026-11-14 验证）
**影响面**：质量门禁可信度（勾选项语义空泛）、质量指标聚合（轮转后样本「消失」）、
CI 误诊（spawn 错误缺上下文）、快照可判读性（样本规模不可见）
**首次定位**：2026-11-14
**解决方式**：方案 A = 静态证据门禁保持 + 失败可见性配套修复（聚合递归/spawn 诊断/计数约定/样本自检/L1 运行时证据）；语义退化检查层的契约已冻结于 ADR-021（绝不静默放行、失败必须可见），实现随 `scripts/audit_todo.py` 后续迭代落地（见 ADR-021「落地现状」）

> 本文保留为历史记录：上半部分是当时的定位与方案分析（结论仍准确），
> 文末「解决记录」列出实际落地的改动与验证结果。

## 现象

弱模型参与产出与审计时，出现一类「格式合规、语义空泛」的退化：

1. `AGENTS.md` 的 `- [x]` 勾选项证据在 `audit_todo.py` 静态门下通过
   （`path.rs::test_fn` 真实存在），但测试内容与勾选语义无关，或断言为空壳；
2. 质量快照字段提取对缺失/未知值敏感：直读会把「未知」误写成 0，污染
   成功率/成本比率（质量收集脚本的 `as_int` + null 兜底即为该问题的防护，
   约定「未知 ≠ 0，不伪造」）；
3. 文档中的单测计数为硬编码数字，随演进漂移（如 T4.1 行 `sisyphus-metrics::quality`
   12 单测 → 实测 21、bridge quality_record 6 → 7，提交 666100f 修正）；
4. 轮转策略落地后（`records/YYYY-MM/` 子目录），`aggregate_dir` 扁平读取，
   轮转后样本「消失」，顶层比率不变但分母偷偷变小；
5. `SandboxdClient::spawn` 失败只报通用 IO 错误，CI 上无法区分
   路径不存在 / 无执行权限 / PATH 查找失败。

## 根因链路（已逐环核实）

1. 门禁只核对「证据存在」，未核对「证据语义」→ 弱模型可钻格式空子；
2. 提取路径缺容错 → 缺失字段被误填为 0 而非 null；
3. 计数写死 → 演进即漂移（文档与实跑脱节）；
4. 聚合未递归 → 轮转目录不在采样范围内；
5. 错误映射丢上下文 → `format!("{e}")` 未附路径/存在性/errno。

## 解决记录

| 改动 | 位置 | 验证 |
|---|---|---|
| 退化审计契约（绝不静默放行、失败必须可见、文档计数约定） | `docs/adr/021-quality-gate-degradation-audit.md` | 本 ADR |
| 快照字段提取「未知 ≠ 0」防护 | `scripts/quality-metrics-collect.sh`（`as_int` + null 兜底） | 快照实跑输出 |
| 聚合递归读取轮转子目录 | `crates/sisyphus-cli/src/handle_metrics.rs` | `aggregates_records_recursively_after_rotation` PASS |
| spawn 诊断附路径/存在性/errno | `crates/sisyphus-bridge/src/sandboxd_client.rs` | `spawn_diagnostic_names_path_and_existence` PASS |
| 文档计数约定 | `AGENTS.md` 约定表 | 全文档相对引用解析核验通过 |
| 快照增样本规模自检 | `scripts/quality-metrics-collect.sh` | `records_count` / `records_bytes` 入快照 |
| L1 运行时证据自动化 | `scripts/single-node-acceptance-evidence.sh --run-l1` | health/ready/sops/cluster/metrics 契约/readiness 全绿 |

## 未实施项（环境阻断）

- **P0-9 云端凭据路径**：无云端凭据且沙箱网络受限，无法实施；维持本地证据链
  （本文件 + ADR-021）作为 interim 状态。凭据可用后按下方「换模型时复检清单」补验。

## 验证（实跑输出摘录，2026-11-14）

```
$ cargo test -p sisyphus-cli --bin sisyphus aggregates_records
test handlers::handle_metrics::tests::aggregates_records_from_directory ... ok
test handlers::handle_metrics::tests::aggregates_records_recursively_after_rotation ... ok

$ cargo test -p sisyphus-bridge --lib spawn_diagnostic
test sandboxd_client::tests::spawn_diagnostic_names_path_and_existence ... ok

$ ./scripts/single-node-acceptance-evidence.sh l1-verify --run-l1
### GET /health            exit: 0   {"status":"SERVING","version":"0.4.0"}
### GET /health/ready      exit: 0   {"components":{...},"status":"ok",...}
### GET /api/sops          exit: 0   ["grpc_sop_e2e_smoke",...]
### GET /api/cluster/status exit: 0  {"cluster_name":"_sisyphus-bridge",...}
### /api/metrics contract  exit: 0   sisyphus_* 指标行数: 13
                                      sisyphus_http_requests_total 0
                                      sisyphus_token_budget_usage 0
### single-node-readiness-check  exit: 0  == OK ==
```

## 换模型时复检清单

- [ ] `make todo-audit` 无新增未证勾选（ratchet 只减不增）；
- [ ] `cargo test -p sisyphus-cli --bin sisyphus handle_metrics` 全绿；
- [ ] `cargo test -p sisyphus-bridge --lib spawn_diagnostic` 全绿（纯函数单测，环境无关；
  daemon 相关测试需 dedicated runner，见仓库惯例）；
- [ ] `./scripts/single-node-acceptance-evidence.sh <tag> --run-l1` 全绿；
- [ ] 新快照 `records_count` 与实跑记录数一致（未知 ≠ 0，不伪造）。
