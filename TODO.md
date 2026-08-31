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
- [ ] **Otel 导出端点配置化**: 当前 otel 配置散落，`OpenTelemetryConfig` 缺少完整 env override 链
- [ ] **认知循环 v2 异常检测器调优**: `AnomalyDetector` 阈值硬编码，需要从 Config 加载
- [x] **bridge 启动自检端点** (2026-08-31): `GET /health/deep` 报告 session_db/shared_http/kernel 组件级状态；返回 200/207 Multi-Status；OpenAPI 文档 + HealthDeepResponse schema
- [x] **`/health/deep` 单元测试** (2026-08-31): 5 个测试覆盖 DB 文件/父目录/不存在/absent 不算降级/三组件键（重构 health_deep_handler → 薄包装 + 纯函数 health_deep_assess）
- [ ] **TUI `:health` 面板**: 整合健康摘要 + token 预算 + DAG 进度

#### 7.2 测试与质量
- [ ] **bridge 集成测试 CI 化**: `sisyphus-bridge/tests/bridge_integration_test.rs` 当前 `#[ignore]`
  - 解法: 拆分为 CI-friendly 子集（无需 mDNS / 网络）
- [ ] **混沌测试扩展**: 当前仅 fault injection 覆盖，缺网络延迟/超时/并发竞态场景
- [ ] **mutation testing 试点**: `cargo-mutants` 在 `sisyphus-types` 跑一轮，验证测试质量
- [ ] **wasm 沙箱的 proptest**: `sisyphus-sandbox` 缺属性测试，添加 host function 边界覆盖

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
- [ ] **部署脚本**: 缺 systemd unit / Docker compose / k8s manifest

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

**下一步（用户可指派优先级）**:
- 7.1: Otel 端点配置化 / AnomalyDetector 阈值化 / TUI `:health` 面板
- 7.2: bridge CI 化 / mutation testing / wasm proptest / 混沌扩展（需 2-4h 调研）
- 7.3: 基准基线刷新 / DAG 渲染性能 / TUI 冷启动 profile
- 7.5: mdbook 缓存 / 部署脚本（systemd / Docker compose / k8s）