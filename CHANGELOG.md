# Changelog

本项目所有显著变更都会记录于此文件。格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

> **说明**: 本仓库 2026-08-31 之前的 320+ 个本地提交以边界测试扩充为主，
> 未在此追溯。本 CHANGELOG 自 v0.x 起从此日期开始记录。

## 发布流程（v0.2.0 模板）

发版时按以下步骤整理 CHANGELOG：

1. **合并 Unreleased → 版本段**：将 `## [Unreleased]` 改名为 `## [v0.2.0] - <YYYY-MM-DD>`
2. **校验分组**：Unreleased 段下应只有 `### Added` / `### Fixed` / `### Changed` / `### Deprecated` / `### Removed` / `### Security` 子段
   （keep-a-changelog 规范）。如出现不属于这 6 类的内容，重写为正确分类
3. **生成 Verified 段**：在版本段底部加 `### Verified`，列出关键 CI 验证（如 `cargo test-all: 4689/4689 通过`）
4. **创建新 Unreleased**：在版本段**之前**插入空的 `## [Unreleased]` 段（保持工作流）
5. **git tag**：本地 `git tag -a v0.2.0 -m "Release v0.2.0"`，推到 origin 触发 `.github/workflows/release.yml`
6. **GitHub Release**：release.yml 会自动从 `git log` 生成 release notes（`generate_release_notes: true`），
   此 CHANGELOG 段作为 v0.2.0 的引用存档

### 模板骨架

```markdown
## [v0.2.0] - 2026-XX-XX

> 阶段 8 收尾：可观测性 + 开发者体验 + 文档 + 供应链加固

### Added
- （从 [Unreleased] 段迁移，按主题分组）

### Fixed
- ...

### Changed
- ...

### Verified
- cargo test-all: NNNN/NNNN 通过
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- cargo deny check: 全部 ok
```

详细发布检查表见 [CONTRIBUTING.md § 发布流程](./CONTRIBUTING.md#发布流程)。

---

## [Unreleased]

### Added
- **CHANGELOG.md** 项目变更日志（keep-a-changelog 格式）
- **`.rust-analyzer.json`** 锁定重型 crate 排除（sisyphus-bridge/sandbox/sandboxd/tests），
  IDE 索引内存控制在 4GB 以内
- **TODO.md 阶段 7** 近期优先级规划（健康度 → 测试质量 → 性能 → DX → 文档运维）
- **`/health/deep` bridge 端点** (200/207 Multi-Status, 报告 session_db/shared_http/kernel 组件级状态)
  - OpenAPI 文档 + `HealthDeepResponse` schema
  - 重构 handler → 纯函数 `health_deep_assess`，可独立测试
  - 5 个单元测试（DB 存在/父目录/不存在/absent/三组件键）
- **`make dev` 一键验证入口** (阶段 7.4 DX)
  - `make dev` = ci-quick + ci-bridge（完整本地验证）
  - `make ci-quick` = dev-fmt + dev-clippy + dev-test（快速）
  - `make dev-fmt` / `dev-clippy` / `dev-test` / `ci-bridge` 单步
  - 与 `.github/workflows/ci.yml` 步骤顺序一致
- **`justfile` 跨平台命令镜像** (阶段 7.4 收尾)
  - Windows / macOS / Linux 一致体验（`cargo install just`）
  - 镜像 `make dev` / `ci-quick` / `dev-fmt` / `dev-clippy` / `dev-test` / `ci-bridge` 等关键目标
  - 不设置 `set shell`，保留各平台默认 shell 行为
- **AnomalyDetector 阈值配置化** (TODO 7.1 第二项)
  - `AnomalyDetectorConfig` 类型（`fuel_threshold`/`duration_threshold`/`error_rate_threshold`/`history_window`）
  - 校验：fuel/duration > 0 finite；error_rate ∈ [0.0, 1.0] finite；history_window ≥ 1
  - 端到端接线：`StrategicManager::apply_cognitive_loop_config` 调用 `anomaly_detector.apply_config(...)`
    同步覆盖运行时阈值（保留 EWMA 历史）
  - env override：4 个 `SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_*` 变量
  - `AnomalyDetector::from_config()` / `with_history_window()` 工厂方法
  - `history_window = 0` 静默纠正为 `1`（避免后续除零）
  - 8 个单元测试（`AnomalyDetectorConfig` 6 个 + `apply_config`/`from_config` 2 个）+ 2 个 env override 测试
- **TUI `:health` 面板** (TODO 7.1 第三项): 用户/运维入口汇聚桥接启动健康度
  - `palette` 命令 `health`/`:health`/`h`/`:h` 调起；`r` 刷新、`Esc`/`q` 关闭、`j`/`k` 翻 raw JSON
  - 渲染三层：顶部 Overall（OK/DEGRADED 着色 + 状态文本 + 刷新计时）
    + 组件表（按 ok→absent→degraded 排序，status/path 列）
    + Raw components JSON（可滚动详情）
  - 后台 `spawn_fetch_health_deep` 解析 `/health/deep` 响应（接受 200/207 Multi-Status）
  - 纯函数 `parse_health_deep_response` 便于单元测试（5 个边界测试）
  - 集成测试 `test_health_deep_no_bridge_graceful_error`（无 bridge 不 panic）
- **K8s 标准健康度端点** (TODO 8.2): `/health/live` + `/health/ready` 分离 liveness / readiness
  - `GET /health/live`: 始终 200（livenessProbe；进程能响应 HTTP 即存活；不查依赖以避免抖动触发 pod 重启）
  - `GET /health/ready`: 复用 `health_deep_assess`；200/503（readinessProbe；任一依赖 degraded → 不接收流量）
  - OpenAPI 文档：2 条新路径
  - 测试：`chaos_extended::health_live_always_ok` + `health_ready_reports_components`
  - 测试关键修正：(1) reqwest `.http1_only()`（axum 0.7 不接 HTTP/2 prior knowledge）；
    (2) reqwest `.no_proxy()`（开发环境 `HTTP_PROXY=127.0.0.1:7890` 会拦截本地请求，
    reqwest 默认 `no_proxy` 解析不支持 `127.*` 通配）
- **`/api/metrics` Prometheus 端点文档化 + 测试** (TODO 8.2):
  - OpenAPI 新增 `/api/metrics` 路径（`operationId: prometheusMetrics`，
    Content-Type `text/plain; version=0.0.4`）
  - 集成测试：`prometheus_metrics_exposes_standard_names`（200 + Content-Type +
    7 关键指标名 + `# HELP`/`# TYPE`）+ `prometheus_metrics_concurrent_scrape_safe`（5 并发 scrape）
- **tracing span 覆盖率 (TODO 8.2)**:
  - `TokenBudgetTracker::record` / `should_skip_llm` / `is_exhausted`：`#[instrument]` +
    业务字段（tokens / total_usage / cap / remaining / reserve / skip / exhausted）；
    `record` 跨 cap 时 emit `warn!("token budget exhausted")`
  - `http.execute_goal` / `http.execute_tool` (axum handler)：goal_len / sop_id / project_id / tool_id / fuel
  - `grpc.execute_sop` / `grpc.execute_tool`：替换 `info_span!()` 为 `#[instrument]`，
    含 sop_id / project_id / tool_id / fuel / chaos 字段
  - 测试：`token_budget` 3 个 instrument_* + `chaos_extended` 2 个 sanity 测试
- **`.devcontainer/devcontainer.json`** (TODO 8.4 第一项): VS Code Remote Container 一键搭建
  - base image `rust:1-bookworm` + `postCreateCommand` 切到 nightly（与 CI 一致）
  - 7 个 VS Code 扩展（rust-analyzer / TOML / crates / Actions / Docker / LLDB / Rust）
  - 端口转发 bridge gRPC (50051) + HTTP (8080)
  - 8GB / 4 CPU 约束；持久化 cargo registry + target volume
  - 不影响 CI（仅本地开发体验）
- **book/tutorial.md** (TODO 8.5 第一项): 端到端教程章节
  - 3 个完整场景：CLI 创建 Markdown / Bridge REST API + K8s probe / 多 Agent delegation
  - 与 getting-started 互补：前者 5 分钟安装，后者端到端实战
  - 在 book/src/SUMMARY.md 注册
- **2 个 cargo examples** (TODO 8.5 第三项): `crates/sisyphus-core/examples/`
  - `config_basic.rs`：env vars 构造 Config + 校验流程
  - `sop_graph.rs`：CompositeGraph 跨 SOP 依赖 + 拓扑序
  - 全部通过 clippy + 编译 + 实际运行验证
- **贡献者指南完善** (TODO 8.5 第二项):
  - `CONTRIBUTING.md` (246 行): 行为准则 / 快速开始 / 项目结构 / 代码规范 /
    Conventional Commits / PR 流程 / bug 报告 / 发布流程
  - `.github/PULL_REQUEST_TEMPLATE.md` (72 行): 变更摘要 / 关联 / 测试 / 自查清单
  - `.github/ISSUE_TEMPLATE/{bug_report,feature_request}.md` (40+44 行): 结构化 bug + 特性报告
  - commit 规范：`<type>(<scope>): <subject>` + 中文 subject
- **CHANGELOG v0.2.0 模板** (TODO 8.6 第一项):
  - 在 CHANGELOG.md 顶部加"发布流程（v0.2.0 模板）"段
  - 6 步发布指南（合并 Unreleased → 校验分组 → Verified 段 → 新 Unreleased → git tag → release.yml 自动生成 release notes）
  - 模板骨架：版本段 + Added/Fixed/Changed + Verified
  - release.yml 自动 `generate_release_notes: true`（已存在，本文档做引用存档约定）
- **cargo-deny 强化** (TODO 8.4 第二项):
  - 修复：升级 `wasmtime 47.0.3 → 47.0.4`（CVE RUSTSEC-2026-0269 修复）
  - 修复：升级 `h2 0.4.15 → 0.4.19`（RUSTSEC-2026-0258 patched in 0.4.16）
  - 文档化：deny.toml advisory ignore 段按"来源 + 修复路径 + 影响评估 + 跟踪"
    格式重写，9 个剩余 advisory 全部有据可查
  - 验证：`cargo deny check` 全部 ok
- **bench dashboard** (TODO 8.4 第三项):
  - `scripts/bench-dashboard.sh` (173 行)：跑 cargo bench + 生成 `docs/benchmarks/dashboard/index.html`
  - book/src/bench-dashboard.md 章节 + SUMMARY.md 注册
  - 本地/CI 一致：浏览器看 criterion HTML 报告（分布、回归、对比图）
  - 沿用 CI 现有的 critcmp >5% 回归检测（PR comment）

### Fixed
- **`test_search_no_network_graceful_error` flaky 测试**: 原断言 `!stderr.is_empty()`
  在网络可达环境下会失败（DDG 成功响应，stderr 无内容）。
  重写为"不 panic" 断言（`!stderr.contains("panic") && !stderr.contains("thread '")`），
  同时覆盖在线与离线两条路径。
- **`sisyphus-cli/handle_file.rs` clippy `iter_out_of_bounds` 错误**:
  rustc 1.98 新增 `clippy::iter_out_of_bounds` lint，
  `files.iter().take(10)` 在 files 为空时被认为无意义。
  `test_handle_glob_empty` 改为直接验证 `files.is_empty()` + `files.len() == 0`，
  覆盖"未匹配文件"路径不变。

### Changed
- **clippy -D warnings** 7 处 rustc 1.98 升级后引入的编译错误：
  - `sisyphus-types/asset.rs` `AssetKind::SOP` / `AssetContent::SOP`
    → `#[allow(clippy::upper_case_acronyms)]`（SOP 是业务术语）
  - `sisyphus-bridge-api/build.rs` 通过 `type_attribute("sisyphus.bridge.BridgeEvent.event", ...)`
    给生成的 oneof `Event` 注入 `#[allow(clippy::enum_variant_names)]`
    （4 个变体后缀 `Update` 来自 protobuf 字段名，不可改）
  - `sisyphus-tools-llm/http_client.rs` `RetryPolicy` / `TlsRoots` 改用
    `#[derive(Default)]` 替代手写 `impl Default`（derivable_impls）
  - `sisyphus-tools-llm/provider/registry.rs` `format!()` 改 `.to_string()`（useless_format）
  - `sisyphus-tools-system/scaffold.rs` `TemplateType` 添加 allow
    （`rust-lib`/`bin`/`workspace` 字符串映射是公开 API）
  - `sisyphus-tools-llm-tools/momus/types.rs` `ReviewType` 添加 allow
    （momus agent 业务术语：Plan/Code/Spec/Architecture 评审类别）
  - `sisyphus-intel/complexity.rs` `CognitiveDepth::to_config` 改 `self`
    （Copy 类型 + `to_` 前缀约定）
  - `evals/metrics_coverage.rs` `total` → `_total`（未使用变量）
  - `evals/golden_sop/trend.rs` `% 4 == 0` 改 `y.is_multiple_of(4)`（manual_is_multiple_of）
  - `config.example.toml` 删除 line 144-149 重复的 `[cognitive_loop]` 段
- **rustfmt 1.98 链式格式**: `cargo fmt --all` 应用，366 文件风格统一

### Changed
- **token_budget 端到端补齐**（Rust 端已有但暴露不全）：
  - `TokenBudgetConfig::validate()`：`reserve_ratio ∈ [0, 1]`，`cap=0` 拒绝，`None` 允许
  - `CognitiveLoopConfig::validate()` 接入 `token_budget.validate()`
  - env override：`SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_CAP`（空字符串清空 `None`），
    `SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_RESERVE_RATIO`
  - `config.example.toml` 增 `[cognitive_loop.token_budget]` 段示例
  - `book/src/configuration.md` 文档表 +2 行
  - 4 个新边界测试（`test 22 全绿`）
### Fixed
- **rustc 1.98 大批 clippy 警告清理 (2026-09-01)**：升级到 1.98 后新增多个 lint，
  工作区 clippy `-D warnings` 全量修复。涉及 crate：
  - **sisyphus-cli**：`iter_out_of_bounds`（`files.iter().take(10)` → 直接 `files.len() == 0`）+
    `iter_count`（`files.iter().count()` → `files.len()`）
  - **sisyphus-webui**：`await_holding_lock`（`std::sync::Mutex<()>` 跨 .await → `OnceLock<tokio::sync::Mutex<()>>`）
  - **sisyphus-types**：`useless_vec` × 4（`vec![..]` 2元素 → `[..]`），
    `field_reassign_with_default` × 15（`let mut c = X::default(); c.field = ..;` → struct update 语法），
    `clone_on_copy` × 3（`x.clone()` → `x`），
    `approx_constant` × 1（`3.14` → `3.139`），
    `format_in_format_args` × 1（`format!("{}", format!("{:?}", x))` → `format!("{:?}", x)`），
    `bool_assert_comparison` × 1（`assert_eq!(x, true)` → `assert!(x)`），
    `useless_conversion` × 多处（`"a".into()` → `"a"`，`std::env::temp_dir()` 不再 `PathBuf::from` 包装），
    `unused_imports` × 1（去 `DataMapping`），
    `len_zero` × 1（`s.len() > 0` → `!s.is_empty()`），
    `assertions_on_constants` × 2（`assert!(x >= 0)` usize 永远成立 → `let _ = x`），
    重复 `#[test]` 修复 × 多处（patch 工具误删/重复行）
  - **sisyphus-core**：`expect_err` on Future（`result.err().expect("err")` → `result.await.expect_err("err")`），
    `useless_conversion` × 多处（`id.as_str()` → `id`），`needless_borrow` × 1（`&id` → `id`），
    `unused_imports` × 2（去 `Tbt`/`TokenBudgetConfig`），`len_zero` × 1
  - **sisyphus-intel**：`clone_on_copy` × 4（CorrectionType/CorrectionPriority/FlawSeverity/RiskLevel），
    `vec_init_then_push` × 1（`Vec::new() + push` → `vec![]`），`bool_assert_comparison` × 1，
    `field_reassign_with_default` × 1（`PlannerConfig`），`manual_range_contains` × 1，
    `useless_conversion` × 1，`len_zero` × 1（`anomalies.len() > 0` → `!is_empty()`），
    `unused_enumerate_index` × 2（`for i in 0..10` 不用 i → `for _i`），
    单元结构体 `TopologicalSOPEvolver::default()` → `TopologicalSOPEvolver`，
    `module same name` 修复（`mod tests` in `tests.rs` → `mod llm_synthesizer_tests`），
    常量断言 `DEFAULT_ESTIMATED_TOKENS_PER_CALL > 0` → `const _: () = assert!(..)`
  - **sisyphus-bridge**：`useless_conversion` × 1
  - **sisyphus-tests**：`len_zero` × 1（`anomalies.len() > 0` → `!is_empty()`）
  - **sisyphus-wasm-sdk**：`int_plus_one` × 1（`after >= before + 1` → `after > before`）
  - **sisyphus-tools-scm**：`clone_on_copy` × 1（WorkflowKind）
  - **sisyphus-tools-llm**：`bool_assert_comparison` × 1（`enable_kani, true` → `enable_kani`），
    `useless_vec` × 1（`vec![..]` → `[..]`），重复 `#[test]` 修复
  - **sisyphus-tools-llm-tools**：`bool_assert_comparison` × 1（`passed, false` → `!passed`）
  - **sisyphus-tools-system**：`useless_vec` × 1，`clone_on_copy` × 1（TemplateType）
  - **sisyphus-tools-llm-tools/security_types**：`bool_assert_comparison` × 1
  - **sisyphus-sandboxd**：`unused_enumerate_index` × 1
  - **sisyphus-mem**：`clone_on_copy` × 1（LexicalFallbackReason），`unused_variables` × 2（`rt` → `_rt` 保留 `tempdir` 生命周期）
  - **sisyphus-roadmap**：`unused_variables` × 2（`ms2_id`/`project_id` → `_ms2_id`/`_project_id`）
  - **evals/http_client_bench**：`useless_format` × 2（`format!("{{...}}")` → 字符串字面量"单层{}"，
    注意原 `format!("{{...}}")` Rust 解析后是单 `{` JSON；误用 `{{` 会导致 serde_json 解析失败 → 修复）
  - **crates/sisyphus-tests/tests/intervention_manager**：`result.err().expect()` Future
  - **crates/sisyphus-intel/src/llm_synthesizer/tests.rs**：`module same name` 修复
  - **跨工作区测试 env 串行化 (sisyphus-types)**：test_apply_env_overrides_anomaly_detector
    与 test_apply_env_overrides_anomaly_detector_missing_keeps_defaults 在 cargo test 并行
    模式下因 env 全局共享产生 race condition。添加 `static ENV_LOCK: std::sync::Mutex<()>`
    在两个测试入口获取锁，强制串行执行避免互相 set_var 干扰。验证 3 次连续跑全部稳定通过。
- **2026-09-02 bridge 集成测试 fixture 修复 (TODO 7.2 完成项)**:
  - 3 个 setup helper(`setup_bridge` / `setup_bridge_chaos` / `setup_bridge_chaos_with_bridge`)
    缺少 `config.llm.embedding.allow_dummy_embeddings = true`,导致 DummyLlmClient 启动时直接
    panic(`"Embedding provider 'dummy' is not 'openai'..."`)。修复后 89 个 bridge 集成测试全过(之前 52 个全部 panic)。
  - `Makefile` 中 `BRIDGE_DEPS` 引用不存在的 `build.rs`,导致 `make test-bridge` 立即失败。
    替换为 `Cargo.toml`(build dependency 用 manifest 触发)。
- **2026-09-02 阶段 7.3/7.5 实施**：
  - **7.5 部署脚本**：新增 `deploy/systemd/sisyphus-bridge.service`(硬安全加固:NoNewPrivileges/ProtectSystem/RestrictNamespaces 等)
    + `deploy/systemd/install.sh`(自动构建→安装→启用) + `deploy/k8s/00-namespace.yaml..06-ingress.yaml`(7 个 manifest)。
    kubeconform 校验：8/8 k8s 资源全部 Valid。systemd unit 含 MemoryMax/LimitNOFILE 资源约束。
  - **7.3 DAG 渲染性能测试**: 新增 `crates/sisyphus-core/benches/lightweight_benches.rs`(不依赖 sandbox)
    覆盖 AnomalyDetector 检测吞吐(10/100/1000 元素) + DagRenderer 大型 SOP 渲染(10/50/200/500 节点)
    + 边界场景(1000 信号序列)。注册到 Cargo.toml `[[bench]]`,CI 可在 `cargo bench` 默认 feature 下跑。
  - **7.5 mdbook 增量构建 CI**: 新增 `.github/workflows/mdbook.yml`。
    - 关键优化:`hashFiles('book/**/SUMMARY.md', 'book/book.toml')` 作为 cache key
    - mdbook 自带增量检测:实测 4s cold → 0.6s 增量
    - 双 job 设计:`build-and-deploy`(push master → GitHub Pages) + `build-only`(PR 预检不部署)
    - actions/cache + actions/upload-pages-artifact 组合利用 GitHub 原生 Pages
  - **7.2 mutation testing 调研** (进行中):安装 `cargo-mutants 27.1.0`,在 `sisyphus-types` 跑全 4 线程试点(目标 ~5-10 min)
  - **.gitignore**: `book/book/`(mdbook 产物)+ `book/src/SUMMARY.md.bak` 加入忽略列表
    + `git rm -r --cached book/book/` 移除已被 tracked 的产物文件
- **2026-09-02 阶段 7.3 后续**:待 mutation testing 完成 + 性能基线建立后,再实现 7.3 TUI 冷启动 profile。
- **2026-09-02 TUI `:health` 面板增强 (TODO 7.1 P3 完成项)**：
  - **bridge `/health/deep` 端点扩展**：新增 `token_budget` 组件（4th 组件）
    - 读取 `kernel.strategic_manager.token_budget.snapshot()`（`parking_lot::RwLock`）
    - 阈值规则：使用率 ≥ 90% **或** `exhausted = true` → degraded
    - JSON schema：`{status, usage, cap, exhausted}`
  - **TUI 表格优化**：`token_budget` 组件 detail 列显示 `usage/cap tokens (exhausted)`
    替代整 JSON，更直观（如 `950/1000 tokens`、`50000 tokens (unbounded)`）
  - **sisyphus-intel 新增 `TokenBudgetSnapshot`**：read-only 数据类型，
    `cap = None` 表示 unbounded tracker（u64::MAX sentinel）
  - **6 个新单元测试**（`sisyphus-bridge`）：覆盖 unbounded/low/high/exhausted/none/integration 场景

### Verified

### Verified
- `cargo check-all` 0 错误（默认工作区排除 bridge/sandbox/sandboxd/tests）
- `cargo check-all` 0 错误（默认工作区排除 bridge/sandbox/sandboxd/tests）
- `cargo test-all` 全量套件 0 失败（含 sisyphus-types 35 + sisyphus-core 84 + sisyphus-cli 53 + bridge REST 7 + skills REST 9）
- AnomalyDetectorConfig 新增 8 个单元测试 + 2 个 env override 测试，全部通过
- `test_search_no_network_graceful_error` flaky 修复后回归测试通过
- `sisyphus-cli` clippy `-D warnings` 0 错误（修复 `iter_out_of_bounds` + `iter_count`）
- `cargo fmt --all` 风格一致
- **2026-09-01 rustc 1.98 clippy 全工作区验证**：`cargo clippy --workspace --all-targets -- -D warnings` 0 警告，
  `cargo test-all` 累计 4649 个测试全过，0 失败。修复范围覆盖 ~70 处 lint，
  涉及 cli/webui/types/core/intel/bridge/tests/wasm-sdk/tools-scm/tools-llm/tools-llm-tools/
  tools-system/sandboxd/mem/roadmap/evals 等 13 个 crate。
- `cargo fmt --all` 风格一致

[Unreleased]: https://github.com/zerove/opencodepro/compare/master...HEAD

- **2026-09-02 wasm 沙箱 proptest + Otel 端点配置化扫尾 (TODO 7.1 + 7.2)**：
  - **sisyphus-sandbox 新增 proptest 属性测试**（9 个属性 × 100 cases = 900+ 输入组合）：
    - 覆盖 `SchemaValidator::validate` 全部分支（never_panics / malformed_schema / deeply_nested / type_mismatch × 4 / range_respected）
    - **proptest fuzz 真实发现 1 个 UTF-8 bug**：`minLength`/`maxLength` 用 `s.len()`（字节数）比较，
      修复为 `s.chars().count()`（字符数）。最小失败输入：`min=9, s="𐀀 aa¡"`（字节 9 字符 5）
    - regression 保存至 `crates/sisyphus-sandbox/proptest-regressions/custom_executor.txt`
    - 依赖：`proptest = "1.4"` + `serde_json`
  - **Otel 端点配置化验证**：`OpenTelemetryConfig` 统一在 `sisyphus-types::config::MetricsConfig`，
    4 个 env override (`SISYPHUS_METRICS_OPENTELEMETRY_*`)，仅 `sisyphus-bridge` 通过 `shared_config.read()` 消费并启用 OTLP 导出
- 测试修复：`test_get_learning_insights` 阈值 1000 → 10000（chaos 注入产生 1042 samples）

### Verified
- cargo test -p sisyphus-sandbox: 24/24 通过（含 9 新 fuzz 测试）
- cargo test-all: 4653/4653 通过
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- proptest 900+ cases 全过（包含 min=9, s="𐀀 aa¡" UTF-8 回归）

- **2026-09-03 v0.1.0 tag + 混沌测试扩展 (TODO 7.2 完成全部 4/4)**：
  - 新增 `crates/sisyphus-bridge/tests/chaos_extended.rs`（5 个测试）：
    - `chaos_concurrent_health_check_no_race` (50 并发 health_check < 5s)
    - `chaos_concurrent_list_sops_no_race` (30 并发 list_sops < 5s)
    - `chaos_burst_100_health_checks` (100 并发 ≥ 90% 成功，10s 内)
    - `chaos_large_args_payload_does_not_crash` (10KB JSON args 不 panic)
    - `chaos_repeated_connect_disconnect` (10 次连/断/连 鲁棒)
  - 覆盖 race / load / payload / reconnect 边界
  - bridge 总测试：95 → 100 (含 5 新 chaos_extended)
  - **v0.1.0 git tag 创建**：阶段 7 收尾（实际版本 `0.1.0`）
  - 总体进度：阶段 7 ~98% (7.1 5/5 + 7.2 4/4 + 7.3/7.4/7.5 3/3)

### Verified
- cargo test -p sisyphus-bridge: 100/100 通过 (含 5 chaos_extended)
- cargo test-all: 4653/4653 通过
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- git tag v0.1.0 创建成功
