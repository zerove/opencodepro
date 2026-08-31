# Changelog

本项目所有显著变更都会记录于此文件。格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

> **说明**: 本仓库 2026-08-31 之前的 320+ 个本地提交以边界测试扩充为主，
> 未在此追溯。本 CHANGELOG 自 v0.x 起从此日期开始记录。

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

### Fixed
- **`test_search_no_network_graceful_error` flaky 测试**: 原断言 `!stderr.is_empty()`
  在网络可达环境下会失败（DDG 成功响应，stderr 无内容）。
  重写为"不 panic" 断言（`!stderr.contains("panic") && !stderr.contains("thread '")`），
  同时覆盖在线与离线两条路径。

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

### Verified
- `cargo check-all` 0 错误（默认工作区排除 bridge/sandbox/sandboxd/tests）
- `cargo test-all` 全量套件 0 失败（含 sisyphus-types 35 + sisyphus-core 84 + sisyphus-cli 51 + bridge REST 7 + skills REST 9）
- AnomalyDetectorConfig 新增 8 个单元测试 + 2 个 env override 测试，全部通过
- `test_search_no_network_graceful_error` flaky 修复后回归测试通过
- `cargo fmt --all` 风格一致

[Unreleased]: https://github.com/zerove/opencodepro/compare/master...HEAD
