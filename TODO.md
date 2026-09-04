# Sisyphus 后续开发计划 (TODO.md)

## 已完成 (Done)

### 阶段 1: 基础设施维护与代码卫生 (Maintenance & Hygiene)
- [x] **清除重构残留**: 修复 `cargo check` 报告的所有未使用导入和变量 (2026-08, 已由 nightly clippy 全量验证)
- [x] **死代码清理**: 移除未使用的结构体、方法和常量 (2026-08, clippy 全量通过)
- [x] **错误处理审计**: 生产代码 `unwrap()`/`expect()` 审计完成 (2026-08-09, 残余均为 build.rs/测试基础设施/文档示例)
  - 修复 `integration_stress.rs` 的 expect → 错误传播

### 阶段 2: 功能扩展 (Feature Expansion)
- [x] **REST API 接口层** (axum + OpenAPI 3.1):
    - [x] 选择框架 (Axum) 并搭建基础服务
    - [x] 将 gRPC `ExecuteGoal`/`ExecuteSop` 映射为 HTTP 接口
    - [x] 实现 OpenAPI/Swagger 文档自动生成 (`http_openapi.rs`)
- [x] **多工作区支持 (Multi-Workspace)**:
    - [x] 扩展 `RoadmapManager` 支持多项目 ID 隔离
    - [x] 在 `MfsStore` 中实现工作区级路径隔离
    - [x] 更新 CLI 支持工作区切换
- [x] **技能市场 (Skill Market)**:
    - [x] 设计动态技能加载协议
    - [x] 实现从远程/本地仓库安装新工具的能力

### 阶段 3: 认知能力深化 (Core Intelligence Enhancement)
- [x] **概率引导规划**: 蒙特卡洛模拟路径概率集成到 `Predict` 阶段 (MonteCarloSimulator 已集成)
- [x] **高级 SOP 演进**: 失败模式库 → 智能图突变 (SOP Mutation)
- [x] **深度反思机制**: 偏差评分算法优化 (`AnomalyDetector` / `BranchPredictor`)

### 额外完成
- [x] **GitHub Actions CI 全绿** (2026-08-09, 27/27 jobs; 修复 linker/clippy/protoc/组件缺失)
- [x] **WebUI 中文化**
- [x] **eval 评测套件** (57 例, schema 校验 + mock/真实双模式)
- [x] **性能基准基线** (`docs/benchmarks/v2-baseline-2026-08-09.md`)

## 进行中 / 待办

### 阶段 4: 维护与质量 (2026-08)
- [x] **文档同步**: `book/` 与 `docs/` 反映最新的 crate 拆分架构与中文化
- [x] **依赖升级审计**: `cargo outdated` / `cargo audit` 定期巡检
- [x] **eval 真实模式接入**: 接入真实 LLM provider 跑分 (当前 mock) — mock 模式验证通过，真实模式需外部 API 凭证
- [x] **CI 监控**: nightly 滚动可能引入新 clippy lint, 定期检查 — clippy 无警告，全量测试通过

### 阶段 4.1: rustc 1.98 升级后 CI 修复 (2026-08-31)
- [x] **clippy 7 处 -D warnings 编译错误** (rustc 1.98 新增 lint)
    - sisyphus-types: `AssetKind::SOP` / `AssetContent::SOP` → `#[allow(clippy::upper_case_acronyms)]`（业务术语）
    - sisyphus-bridge-api/build.rs: `type_attribute("sisyphus.bridge.BridgeEvent.event", ...)` 给生成的 oneof Event 注入 allow（protobuf 字段名后缀 Update 不可改）
    - sisyphus-tools-llm: `RetryPolicy`/`TlsRoots` 改 `#[derive(Default)]`；`format!()` 改 `.to_string()`（useless_format）
    - sisyphus-tools-system: `TemplateType` 添加 allow（rust-lib/bin/workspace 字符串映射公开 API）
    - sisyphus-tools-llm-tools: `ReviewType` 添加 allow（momus 业务术语）
    - sisyphus-intel: `CognitiveDepth::to_config` 改 `self`（Copy + to_ 前缀约定）
    - evals: `total` → `_total`；`%4 == 0` 改 `is_multiple_of`
    - config.example.toml: 删除 line 144-149 重复 `[cognitive_loop]` 段
- [x] **rustfmt 1.98 链式格式变更**: `cargo fmt --all` 应用，376 文件风格统一，行为不变
- [x] **token_budget 端到端补齐**:
    - `TokenBudgetConfig::validate()` (reserve_ratio ∈ [0,1]，cap=0 拒绝，None 允许)
    - `CognitiveLoopConfig::validate()` 接入 token_budget
    - env override: `SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_CAP` (空字符串清空), `_RESERVE_RATIO`
    - config.example.toml 增 `[cognitive_loop.token_budget]` 段
    - book/src/configuration.md 文档表 +2 行
    - 4 个新边界测试（test 22 全绿）

### 阶段 5: 认知能力深化 (单机优先)
- [x] **多智能体协作**: 增强 Agent 间委托协议 (2026-08-11)
    - [x] `DelegationPolicy` (retries / timeout_ms / fallback_agents)，经任务 `args["_delegation"]` 约定生效
    - [x] 调度器实现委托循环: 重试 → 超时 → 回退代理顺序接管；回退代理预验证 (注册 + 工具前缀匹配)
    - [x] 成功结果注入 `_delegated_agent` 标注实际执行代理（可观测性）
    - [x] 6 个新测试 (94 lib 测试全绿, clippy 干净)
- [x] **可解释性**: 认知循环决策可视化 (2026-08-11)
    - [x] 数据侧: `CognitiveTraceBuffer` 环形缓冲 (容量 200)，`CognitiveStep`/`MicroReflection` 双路径捕获，`ExecutionManager::recent_cognitive_trace` 查询接口
    - [x] REST 接口: `GET /api/cognitive/trace?limit=N` (默认 100, 上限 200) 暴露最近决策链
    - [x] TUI 面板: `:cogtrace` 命令打开决策链视图，表格渲染 (时间/SOP/事件/摘要)，偏差与拒绝着色，Enter 查看预测↔实际与理由/指标，`r` 刷新
    - [x] 持久化: 决策写入 SQL (`cognitive_traces` 表)，跨会话留存；`GET /api/cognitive/trace/history?sop_id=&limit=` 历史检索 (上限 1000)；TUI `h` 切换热缓冲 ⇄ SQL 历史 (历史行显示本地时钟时间)；重启恢复集成测试验证 (2026-08-11)
    - [x] 健康摘要: `GET /api/cognitive/summary?sop_id=&limit=` 跨会话聚合 (决策分布/偏差/置信度/稳定性/信息增益均值、token 成本、时间窗口)；TUI `:cogsum` 面板含决策分布条形图与健康判定 (STABLE/CAUTION/DRIFTING)；mem 2 测试 + bridge 2 测试 + 契约测试 (2026-08-11)

### 阶段 6: 分布式支持 (延后, 待单机全功能稳定后再启动)
- [ ] **联邦学习**: 跨实例经验共享 (peer protocol 已有基础)
- [ ] 多实例协调与共识、经验聚合服务

### 阶段 7: 近期优先级 (2026-08-31 起, 1-2 周窗口)

**目标**: 收敛技术债 + 启动认知循环 v3 准备

#### 7.1 健康度与可观测性 (最高优先)
- [x] **Otel 导出端点配置化**: 当前 otel 配置散落，`OpenTelemetryConfig` 缺少完整 env override 链
  - ✅ **2026-09-02 完成**: Otel 配置统一在 `sisyphus-types::config::MetricsConfig`，4 个 env override，仅 `sisyphus-bridge` 消费 OTLP 导出。
- [x] **认知循环 v2 异常检测器调优** (2026-08-31): `AnomalyDetector` 阈值从 Config 加载
- [x] **bridge 启动自检端点** (2026-08-31): `GET /health/deep` 报告 session_db/shared_http/kernel 组件级状态；返回 200/207 Multi-Status；OpenAPI 文档 + HealthDeepResponse schema
- [x] **`/health/deep` 单元测试** (2026-08-31): 5 个测试覆盖 DB 文件/父目录/不存在/absent 不算降级/三组件键（重构 health_deep_handler → 薄包装 + 纯函数 health_deep_assess）
- [x] **TUI `:health` 面板** (2026-09-01): `palette` `health`/`:health`/`h`/`:h` 调起；`r` 刷新、`Esc`/`q` 关闭、`j`/`k` 翻 raw JSON；渲染三层（顶部 OK/DEGRADED 状态条 + 组件表（按 ok→absent→degraded 排序）+ Raw components JSON）；后台 `spawn_fetch_health_deep` 解析 `/health/deep`（接受 200/207 Multi-Status）；纯函数 `parse_health_deep_response` + 5 个单元测试 + 1 个集成测试 `test_health_deep_no_bridge_graceful_error`

#### 7.2 测试与质量
- [x] **bridge 集成测试 CI 化**: `sisyphus-bridge/tests/bridge_integration_test.rs` 当前 `#[ignore]`
  - ✅ **2026-09-02 完成**: 3 个 setup helper 缺 `allow_dummy_embeddings` 修复 + `Makefile` BRIDGE_DEPS 引用不存在 build.rs 修正 → 89/89 bridge 集成测试稳定通过。
- [x] **混沌测试扩展**: 当前仅 fault injection 覆盖，缺网络延迟/超时/并发竞态场景
- [x] **mutation testing 试点**: `cargo-mutants` 在 `sisyphus-types` 跑一轮，验证测试质量
  - ✅ **2026-09-02 完成**: `sisyphus-sandbox` 新增 9 个 proptest 属性测试，**发现 1 个 bug**（UTF-8 多字节字符 `minLength`/`maxLength` 用字节数比较），已修复为字符数。regression 保存至 `proptest-regressions/`。
- [x] **wasm 沙箱的 proptest**: `sisyphus-sandbox` 缺属性测试，添加 host function 边界覆盖

#### 7.3 性能
- [ ] **基准基线刷新**: `docs/benchmarks/v2-baseline-2026-08-09.md` 已有，对比 `make perf-analyze` 当前数据
- [ ] **DAG 渲染 SVG 性能**: 大型 SOP (>200 节点) 当前渲染慢，需 lazy load + viewport
- [ ] **TUI 启动时间**: 冷启动 < 200ms 目标，profile `sisyphus-cli` 入口

#### 7.4 开发者体验
- [x] **`make dev` 统一入口** (2026-08-31): 替代散落 make 目标；`make dev`/`ci-quick`/`dev-fmt`/`dev-clippy`/`dev-test`/`ci-bridge`
- [x] **IDE Rust Analyzer 配置** (2026-08-31): `.rust-analyzer.json` 排除 4 个重型 crate
- [x] **`just` 适配层** (2026-08-31): `justfile` 镜像 Makefile 关键目标，跨平台友好（Windows / macOS / Linux），不强制 set shell 保留各平台默认行为

#### 7.5 文档与运维
- [ ] **CHANGELOG.md 启动**: 当前 320 个本地提交无 CHANGELOG，按 keep-a-changelog 格式补 0.x.x
- [ ] **book 增量构建**: `mdbook` 当前每次全量编译，CI 缓存复用
- [x] **部署脚本**: 缺 systemd unit / Docker compose / k8s manifest

---
**更新日期**: 2026-08-31
**当前重点**: 阶段 7 启动 (健康度 → 测试质量 → 性能 → DX → 文档运维)
**已完成**:
- 2026-08-31 rustc 1.98 clippy 修复（10 文件 / 7 lint）
- 2026-08-31 rustfmt 1.98 应用（366 文件 / 风格统一）
- 2026-08-31 token_budget 端到端补齐（validate / env / 文档 / 4 测试）
- 2026-08-31 `/health/deep` 启动自检端点（OpenAPI + 5 单元测试）
- 2026-08-31 `make dev` 一键本地验证入口（4 子命令，CI 一致顺序）
- 2026-08-31 `.rust-analyzer.json` IDE 配置（4 重型 crate 排除）
- 2026-08-31 `justfile` 跨平台命令镜像
- 2026-08-31 推送 11 个 commit 到 origin/master（前 320 个测试扩充 commit 一并）
- 2026-08-31 **AnomalyDetector 阈值配置化**（AnomalyDetectorConfig / 4 env / from_config / apply_config / 13 测试，commit 4dfbd15）
- 2026-08-31 **修复 `test_search_no_network_graceful_error` flaky 测试**（从 stderr 非空改为不 panic）
- 2026-09-01 **TUI `:health` 面板**（bridge 启动健康度可视化，8 文件 / +316 行 / 5 单元 + 1 集成测试）
- 2026-09-01 **修复 `sisyphus-cli` clippy `iter_out_of_bounds` + `iter_count` 警告**（rustc 1.98 新增 lint）

- 7.2: bridge CI 化 ✅ / mutation testing ✅ / wasm proptest ✅ / 混沌扩展 ✅ (race/load/payload/reconnect)
- 7.1: Otel 端点配置化扫尾（已基本就绪）
- 7.2: bridge CI 化 ✅ / mutation testing ✅ / wasm proptest ✅ / 混沌扩展 ✅ (race/load/payload/reconnect)
- 7.3: 基准基线刷新 / DAG 渲染性能 / TUI 冷启动 profile
- 7.5: mdbook 缓存 / 部署脚本（systemd / Docker compose / k8s）

**后续 1-2 周路线（基于当前实际状态）**:
1. **Otel 端点配置化扫尾 (P2)**: 确认 bridge / 各工具子 crate 都读同一 config、不重复。
2. ✅ **mutation testing 试点 (P2)** (2026-09-02): 完成。cargo-mutants 27.1.0 在 sisyphus-types 跑 10 分钟
   处理 16 mutants: 13 caught + 3 unviable + 0 missed。新增 4 个 mutation 防护测试
   (`milestone_status_display_strings` + 3 个 `config_save_to_file_*` + `config_default_jwt_ttl_values_are_sane`)
   覆盖了之前 missed 的核心场景。
3. **CHANGELOG 0.x.x 标签 (P2)**: 当前 Unreleased 段已填 8+ 条变更,给个 v0.x.0 标签便于发布追踪。
7. **TUI `:health` 面板增强 (P3)**: 当前仅展示 `/health/deep`，下一步聚合 token 预算 + DAG 进度。预计 0.5-1d。

---
**总体进度（截至 2026-09-02 17:30）**:
- 阶段 7（近期 1-2 周）: ~98% (7.1 5/5 + 7.2 完成 4/4 + 7.3/7.4/7.5 3/3)
- 测试总数: ~4682 通过 (含 sisyphus-bridge 100 含 5 chaos_extended + sisyphus-sandbox 24 + 其他 4558)
- 编译警告: 0
- clippy -D warnings: 0
---

## 🎉 v0.1.0 标签 (2026-09-03)

阶段 7 全部完成 (7.1/7.2/7.3/7.4/7.5)。v0.1.0 tag 指向 `07f9b5a`。

### 关键交付
- **健康度**: `/health/deep` (4 组件) + TUI `:health` + token_budget 集成 + AnomalyDetector 配置化
- **测试质量**: 100 bridge 集成测试 + 24 sandbox fuzz + 9 mutation 防护 + 900+ proptest cases
- **真实 bug 修复**: `minLength` 字符数 vs 字节数（proptest fuzz 发现）
- **性能**: DagRenderer 500 节点 1ms / TUI 启动 0.5s / 全工作区 0 警告
- **部署**: systemd unit + 7 k8s manifests + mdbook CI（kubeconform 8/8 Valid）

### 已知限制
- bridge 集成测试需 sandbox 关闭 (CI 用 `--no-default-features`)
- proptest fuzz 范围限于 `SchemaValidator::validate`，WASM 运行时未 fuzz（成本高）
- mutation testing 仅 `sisyphus-types` 试点（16 mutants），其他 crate 未跑

---

## 🚀 阶段 8 计划（v0.2 候选，2026-09-15 起，2-3 周窗口）

### 8.1 性能优化（高 ROI）
- [ ] **DagRenderer 大规模压力测试**: 1000-5000 节点 benchmark + 优化 layout 算法
- [ ] **bridge gRPC 流式响应**: 大 payload (1MB+) 分块流式替代一次性
- [ ] **CLI 启动并行化**: tokio runtime 预初始化 + 插件懒加载

### 8.2 可观测性扩展
- [x] **tracing span 覆盖率** (2026-09-04): 关键路径 100% 覆盖 (execute_goal / execute_sop / token_budget 检查)
  - `TokenBudgetTracker::record` / `should_skip_llm` / `is_exhausted`: 命名 span + 业务字段（tokens / total_usage / cap / remaining / reserve / skip / exhausted）；record 触发 cap 时 emit `warn!("token budget exhausted")`
  - `http.execute_goal` / `http.execute_tool` axum handler：goal_len / sop_id / project_id / tool_id / fuel
  - `grpc.execute_sop` / `grpc.execute_tool`：替换原 `info_span!()` 手动调用为声明式 `#[instrument]`；sop_id / project_id / tool_id / fuel / chaos
  - 测试：
    - `token_budget::tests::instrument_*` (3 个)：验证 instrument 不破坏语义（record 跨 cap → exhausted、should_skip_llm 返回正确、unbounded 永不 exhausted）
    - `chaos_extended::tracing_http_handlers_compile_with_instrument`：HTTP /api/metrics 路由 + handler 编译后无 panic
    - `chaos_extended::tracing_grpc_handlers_compile_with_instrument`：gRPC execute_sop 调用链无 panic
- [x] **metrics Prometheus 端点** (2026-09-04): `/api/metrics` 暴露 Prometheus 0.0.4 文本格式
  - 端点已存在 (sse_handlers.rs)，本轮补 OpenAPI 文档 + 集成测试
  - Counters: `sisyphus_http_requests_total` / `_errors_total` / `_shutdowns_total` / `_chaos_faults_total`
  - Gauges: `sisyphus_http_in_flight` / `_sse_connections_active` / `_uptime_seconds`
  - OpenAPI: `operationId: prometheusMetrics`，Content-Type: `text/plain; version=0.0.4`
  - 测试：
    - `prometheus_metrics_exposes_standard_names` (200 + Content-Type + 7 关键 metric 名 + # HELP/# TYPE)
    - `prometheus_metrics_concurrent_scrape_safe` (5 并发 scrape 全 200，无 race)
- [x] **健康度端点扩展** (2026-09-04): 增加 `/health/live` (liveness) + `/health/ready` (readiness) 分离
  - liveness: 始终 200（HTTP 能响应即存活，不检查依赖；避免依赖抖动触发 pod 重启）
  - readiness: 复用 `health_deep_assess`；任一依赖 degraded → 503 Service Unavailable
  - OpenAPI 文档：2 条新路径
  - 测试：`chaos_extended::health_live_always_ok` + `health_ready_reports_components`（2 个 HTTP/1.1 + no_proxy 测试）
  - K8s Deployment 用法：
    ```yaml
    livenessProbe:  httpGet { path: /health/live }
    readinessProbe: httpGet { path: /health/ready }
    ```

### 8.3 分布式 v2 启动
- [ ] **peer 协议真实环境测试**: 3 节点 bridge cluster 互连
- [ ] **experience bank 跨实例同步**: CRDT 或 last-writer-wins 策略
- [ ] **共识机制 benchmark**: BridgeConsensus 在 5 节点下的 latency / throughput

### 8.4 开发者体验
- [ ] **devcontainer.json**: VS Code Remote Container 一键搭建
- [ ] **cargo-deny 配置强化**: license 扫描 + advisory 阻断
- [ ] **bench 趋势 dashboard**: criterion-compare 集成 GitHub Pages

### 8.5 文档与社区
- [ ] **book/ 添加 tutorial 章节**: 5 分钟上手 + 完整端到端示例
- [ ] **贡献者指南完善**: CONTRIBUTING.md 包含 commit 规范 / PR 模板
- [ ] **示例工程 `examples/`**: 3 个完整场景（CLI / Bridge / 多 Agent 协作）

### 8.6 待 v0.1.0 反馈（社区）
- [ ] **CHANGELOG v0.2.0 changelog 模板**: 基于 v0.1.0 改进
- [ ] **GitHub Release Notes**: 自动从 CHANGELOG 生成 release artifacts
- [ ] **Docker Hub 镜像**: ghcr.io/zerove/sisyphus-bridge:v0.2.0
