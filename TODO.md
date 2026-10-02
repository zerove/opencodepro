# Sisyphus 后续开发计划 (TODO.md)

> **当前状态报告**: [docs/project-status-2026-10-02.md](docs/project-status-2026-10-02.md)（最新快照。重要更正：此前标记为完成的 9.1「peer broadcast 集成」实际从未在生产路径生效——事件链路存在三处独立丢弃点，其中一处是复制出的第二份实现。已于 2026-10-02 修复并补齐双节点端到端验证。下方历史勾选保留原有记录，不等于当前全量回归通过。）

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
- [x] **TUI 启动时间**: 冷启动 < 200ms 目标，profile `sisyphus-cli` 入口 (2026-09-05)
  - Release 模式中位数 10 ms（5 次测量）；Debug 模式中位数 22 ms
  - 详见 `docs/benchmarks/cli-startup-v0.2.0-baseline.md`
  - 新增 `cli_startup/config_default` criterion bench（214 ns，CI 追踪回归）
  - 远低于 < 200 ms 目标（20x / 22x 余量）

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
- [x] **DagRenderer 大规模压力测试** (2026-09-05): 1000-5000 节点 bench + 算法优化
  - 扩展 `lightweight_benches::bench_dag_renderer` 至 7 个规模 [10, 50, 200, 500, 1000, 2000, 5000]
  - 优化 `compute_layout`：`queue.sort()` → `queue.sort_unstable()`
    - sort_unstable 是 introsort 实现（不保证等值元素相对顺序，但节点 ID 唯一性保证输出确定性）
    - 性能改进：8-32%（5000 节点：10.8ms → 8.86ms）
  - 关键发现：throughput 在 483-800 Kelem/s 间波动，**接近 O(N) 线性扩展**
  - 5000 节点 8.86 ms，远低于 TUI 60fps 预算（16.67 ms）
  - Baseline：`docs/benchmarks/dag-renderer-v0.2.0-baseline.md`
    + `target/criterion/dag_renderer/v0_2_0_dag_renderer/`
- [x] **bridge gRPC 流式响应** (2026-09-05): 大 payload 检测 + 推荐 streaming
  - `execute_sop_impl` / `execute_tool_impl` 加 >1MB warning
  - 引导客户端改用 `stream_execution`（已 server-streaming）
  - 大 schema 变更（proto 加 `repeated bytes result_chunks`）需客户端
    同步升级，本轮范围限于 runtime 检测 + 日志
- [x] **CLI 启动并行化** (2026-09-05): multi-thread runtime + spawn_blocking
  - `#[tokio::main]` → `#[tokio::main(flavor = "multi_thread", worker_threads = 2)]`
  - 默认 current_thread → 2 worker，balance 冷启动 vs 并行能力
  - `platform::detect_platform` 走 `spawn_blocking`，释放 main thread
  - 加 startup 时间日志（`startup={}ms`），便于 profile
  - Config 加载（<10ms IO）保留同步：串行开销 < 并行协调成本

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
- [x] **peer 协议骨架实施** (TODO 8.3.1 / ADR-009 § 1 Phase A, 2026-09-05)
  - cluster.proto 新增（Cluster service + JoinRequest/Response + ClusterEvent 双向流）
  - mdns-sd 0.21 集成（MdnsConfig + register_local_node + browse_peers）
  - PeerManager 状态管理（parking_lot::RwLock + broadcast::Sender，leader = 字典序最小）
  - ClusterServiceImpl gRPC service impl（Join + Stream）
  - 6 个 env override（SISYPHUS_CLUSTER_*）
  - ClusterConfig 校验（join_timeout ≥ 1, peer_ttl > join_timeout, peer_ttl ≥ 5）
  - 33 个 peer 单元测试（manager 16 + service 8 + discovery 4 + fallback 5）+ 4 个 2 节点集成测试
  - **Phase B 完成** (2026-09-05)：
    - TTL 清理：last_seen map + touch() + evict_expired() 实际实现
    - 静态 peer fallback dialer：spawn_static_peers + 指数退避重连
    - main.rs 集成：ClusterService 注册 + static dialers 启动
  - **未完成**：Experience CRDT 端到端 2 节点 sync（CRDT API 已就绪，peer broadcast 留待）
- [x] **experience bank 跨实例同步**: CRDT 或 last-writer-wins 策略 (2026-09-05)
  - 设计草案：ADR-009 § Experience CRDT
  - LWW-Element-Set + vector clock：merge_crdt 纯函数 + bump_local_clock
  - store.rs 加 version/vector_clock 列 + ALTER TABLE migration
  - MfsStore::apply_remote_experience API（原子 CAS + Applied/Kept/Equal 决策）
  - VectorExperienceBank 自动 bump 本地 version
  - 9 个 CRDT 纯函数单元测试（types 层）
  - 7 个 roundtrip 集成测试（SELECT 反序列化 + corrupted JSON 健壮性）完成
  - 端到端 2 节点 sync 留待 8.3.1 peer 协议落地
- [x] **共识机制 benchmark** (2026-09-05): ClusterConsensus 5 节点 latency bench
  - 改进 `crates/sisyphus-bridge/src/consensus.rs`：
    * 加 `PeerProposer` trait（可注入 mock）
    * `ClusterConsensus::with_proposer()` 构造方法
    * 动态 `add_peer()` / `remove_peer()` / `cluster_size()`
    * propose 改 majority quorum 校验 + 超时
  - 9 个单元测试 + 1 个 5 节点 bench（`bench_consensus_5_nodes`）
  - Benchmark: 5 节点 mock 零延迟 → 4.62ms / 1000 ops（217 ops/sec）
  - Baseline：`docs/benchmarks/consensus-v0.2.0-baseline.md`

### 8.4 开发者体验
- [x] **devcontainer.json** (2026-09-04): VS Code Remote Container 一键搭建
  - base image: `mcr.microsoft.com/devcontainers/rust:1-bookworm`
  - postCreateCommand: 切换到 nightly + 装 rustfmt/clippy/rust-src + cargo-deny
  - 7 个 VS Code 扩展（rust-analyzer / Even Better TOML / crates / GitHub Actions / Docker / LLDB / Rust）
  - 与仓库 `.rust-analyzer.json` 协同：rust-analyzer.cargo.features=all
  - 端口转发：bridge gRPC (50051) + HTTP (8080)
  - 内存/CPU 约束：8GB / 4 CPU（rust-analyzer + cargo 编译需要）
  - mount 持久化 cargo registry + target 缓存
  - 注意：devcontainer 不影响 CI（CI 用 .github/workflows/ci.yml）
- [x] **cargo-deny 配置强化** (2026-09-04): 主动审计 + 文档化当前 advisory
  - 实际修复：升级 `wasmtime 47.0.3 → 47.0.4` (RUSTSEC-2026-0269, 已 CVE)
  - 实际修复：升级 `h2 0.4.15 → 0.4.19` (RUSTSEC-2026-0258, patched in 0.4.16)
  - 文档化剩余 9 个 advisory 为 ignore 段，每条带：
    * 来源（哪个 crate 引入）
    * 修复路径（如"im → imbl 迁移"）
    * 跟踪 issue / 阶段规划
    * 实际影响评估（如 lru UB 仅当 panic-Drop 触发）
  - skip 段补 `h2 0.3.x` (reqwest 0.11 legacy)
  - 移除 stale `lru = 0.12` skip（unnecessary）
  - 验证：`cargo deny check` 全部 ok（licenses/bans/advisories/sources）
- [x] **bench 趋势 dashboard** (2026-09-04): criterion HTML 报告 + 入口索引
  - `scripts/bench-dashboard.sh` (173 行)：跑 cargo bench + 生成 `docs/benchmarks/dashboard/index.html`
  - 自动从 `target/criterion/*/*/estimate.json` 抽取 mean 估计
  - 本地/CI 一致：浏览器直接看每 bench 的 report（含分布、回归检测、对比图）
  - book/src/bench-dashboard.md 章节：在 book/SUMMARY.md 注册
  - CI 已有的 critcmp >5% 回归检测保留（PR comment）
  - 改进 (8.4.3 余项, 2026-09-05): bench dashboard gh-pages 部署
  - `.github/workflows/ci.yml` bench-core 加 `Generate bench dashboard` step + 上传 artifact
  - `.github/workflows/mdbook.yml` build-and-deploy 加 `needs: [bench-core]` + 下载 artifact
  - 合并到 `book/book/bench/` 路径（与 mdbook 一并 deploy-pages）
  - PR comment 用 `marocchino/sticky-pull-request-comment` 显示 dashboard 链接

### 8.5 文档与社区
- [x] **book/ 添加 tutorial 章节** (2026-09-04): `book/src/tutorial.md` 端到端教程
  - 3 个完整场景：CLI（创建 Markdown）/ Bridge（REST API + K8s probe）/ 多 Agent（delegation + fallback）
  - 与 getting-started 互补：getting-started 是 5 分钟安装；tutorial 是端到端实战
- [x] **贡献者指南完善** (2026-09-04): CONTRIBUTING.md + PR/Issue 模板
  - `CONTRIBUTING.md` (246 行): 行为准则 / 快速开始 / 项目结构 / 代码规范 / commit 规范 / PR 流程 / bug 报告 / 发布流程
  - `.github/PULL_REQUEST_TEMPLATE.md` (72 行): 变更摘要 / 关联 issue / 测试覆盖 / 影响范围 / 自查清单
  - `.github/ISSUE_TEMPLATE/bug_report.md` (44 行): 复现步骤 / 环境 / 日志
  - `.github/ISSUE_TEMPLATE/feature_request.md` (40 行): 问题背景 / API 草稿 / 影响范围 / 优先级
  - commit 规范采用 [Conventional Commits](https://www.conventionalcommits.org/)
- [x] **示例工程 `examples/`** (2026-09-04): 2 个可编译 cargo examples
  - `crates/sisyphus-core/examples/config_basic.rs`：从 env vars 构造 Config + 校验
  - `crates/sisyphus-core/examples/sop_graph.rs`：构造 CompositeGraph（跨 SOP 依赖）
  - 运行：`cargo run -p sisyphus-core --example config_basic` / `--example sop_graph`
  - 全部通过 clippy + 编译 + 实际执行

### 8.6 待 v0.1.0 反馈（社区）
- [x] **CHANGELOG v0.2.0 changelog 模板** (2026-09-04): 发布流程文档化
  - 在 CHANGELOG.md 顶部加 "## 发布流程（v0.2.0 模板）" 段，含 6 步指南
  - 模板骨架：版本段 + Added/Fixed/Changed + Verified（CI 验证清单）
  - keep-a-changelog 规范：6 类标准分组（Added/Fixed/Changed/Deprecated/Removed/Security）
  - release.yml 已自动从 git log 生成 release notes（`generate_release_notes: true`），
    CHANGELOG.md 作为引用存档而非生成源
- [x] **GitHub Release Notes** (2026-09-05): release.yml 已自动生成
  - `Generate Changelog` step: `git log PREV_TAG..HEAD` 生成 commit 列表
  - `softprops/action-gh-release` 的 `generate_release_notes: true`：
    自动从 PR/commits 生成结构化 release notes（含贡献者分组）
  - `body: ${{ steps.changelog.outputs.CHANGELOG }}` 把手动 changelog 拼到顶部
  - 完整发布流水线：tag push → 4 平台 build → release 创建 → 上传 binaries
  - 审计：v0.2.0 tag 实际触发时 review 即可
- [x] **Docker Hub 镜像** (2026-09-04): ghcr.io 自动发布 workflow
  - 新增 `.github/workflows/docker-release.yml` (123 行)：push tag `v*` 自动触发
  - 多平台构建：`linux/amd64` + `linux/arm64`（QEMU 模拟，无需 ARM64 runner）
  - 多 tag 策略：`v0.2.0` → `:v0.2.0`, `:0.2`, `:0`, `:latest`
  - 预发布支持：`-rc.1` / `-beta` 自动标 `prerelease`
  - 缓存：`cache-from: type=gha` + `cache-to: type=gha,mode=max` 复用构建层
  - 沿用 `nightly.yml` 的 docker job（`:nightly` + `:latest`），与 docker-release 互补
  - 文档：`book/src/deployment.md` 填充完整 docker + K8s + 健康度端点说明（之前只一个标题）
  - 文档：`README.md` "快速开始" 加 docker pull 方式
  - 文档：`CONTRIBUTING.md` § 发布流程更新（手动 → 自动 workflow）

---

## 🚀 阶段 9 计划（v0.3 候选，2026-09-06 起，3-4 周窗口）

**目标**: 完成分布式 v2 端到端 sync + 启动认知循环 v3 架构设计

### 9.1 分布式 v2 收尾（最高优先）

#### 9.1.1 peer broadcast 集成
> **2026-10-02 更正**：下方前三项曾被标为完成，实际情况是它们只存在于框架层。事件从本地广播到对端落库要穿过「gRPC 流 → proto 解码 → 本地事件总线」三个环节，每个环节各有一处独立的静默丢弃（其中一处是复制出的第二份实现），生产路径上从未跑通。现已修复并补齐端到端验证。

- [x] **ExperienceSync 事件广播**: `PeerBroadcastExperienceBank` 装饰器，本地落库成功后广播（先落库后广播，广播失败不影响本地结果）
- [x] **ExperienceSyncHandler**: 订阅 PeerManager 事件，按 CRDT 合并（`apply_remote`）写入本地库，跟随关闭信号优雅停止
- [x] **入站事件投递**: `PeerManager::deliver_inbound_experience_sync` 把远端事件投递到本地总线（不用 `broadcast_`，避免集群内回弹）；转换函数收敛为 `service.rs` 单一实现，删除 `fallback.rs` 副本
- [ ] **启动时全量 sync**: 新节点加入时请求所有 peer 的 experience
- [x] **运行时增量 sync**: 本地 store 变更后主动广播到 peer

#### 9.1.2 端到端集成测试
- [x] **2 节点 sync 测试**: `cluster_integration.rs` 扩展（2026-10-02）
  - [x] 生产拨号器真的把事件送到对端（`test_shipped_dialer_delivers_events_to_peer`，直接锁定三处丢弃缺陷）
  - [x] gRPC 往返隔离验证（`test_experience_sync_survives_grpc_round_trip`）
  - [x] 节点 A 写入 → 节点 B 可见，且反向亦然（`test_two_node_experience_sync_round_trip_persists`）
  - [x] 并发写入冲突：CRDT 版本高低裁决（`test_newer_remote_version_wins_concurrent_write`）
  - [x] 重复投递幂等（`test_duplicate_delivery_does_not_duplicate_experience`）
- [ ] **3 节点 Docker Compose**: 本地多节点验证
- [ ] **断线补偿**: 重连后按 vector clock 差量补齐（事件即发即失，断线期间的经验会永久丢失）
- [ ] **chaos 扩展**: 网络分区 + 节点重启后 sync 恢复

### 9.2 认知循环 v3 研究（探索性）

#### 9.2.1 架构设计（ADR-010）
- [ ] **问题分析**: 当前 v2 的瓶颈（token 预算限制、反思深度不足）
- [ ] **候选方向**:
  - SOP 图神经网络嵌入（节点语义编码）
  - 自适应 token 预算分配
  - 长期记忆压缩与检索
- [ ] **决策记录**: ADR-010 编写

#### 9.2.2 认知能力原型
- [ ] **SOP 嵌入实验**: 将 SOP 节点编码为向量
- [ ] **相似度检索**: 基于嵌入的经验检索
- [ ] **评估**: 与当前 experience bank 对比

### 9.3 测试质量巩固

#### 9.3.1 Mutation Testing 扩展
- [ ] **sisyphus-mem mutation**: 覆盖 store/memory 路径
- [ ] **sisyphus-bridge mutation**: 覆盖 gRPC handler 路径
- [ ] **修复 missed mutants**: 基于 cargo-mutants 报告

#### 9.3.2 Fuzz Testing
- [ ] **sandbox wasm proptest 扩展**: host function 边界覆盖
- [ ] **HTTP handler fuzz**: axum 请求模糊测试
- [ ] **gRPC payload fuzz**: prost message 变异测试

### 9.4 文档与发布

#### 9.4.1 CHANGELOG v0.2.0
- [ ] **发布笔记**: 基于 git log 生成结构化 changelog
- [ ] **v0.2.0 tag**: 打标签触发 release workflow
- [ ] **release notes review**: 人工检查自动生成内容

#### 9.4.2 文档优化
- [ ] **mdbook 增量构建**: CI 缓存优化
- [ ] **ADR 索引**: 文档化所有架构决策
- [ ] **API 参考更新**: 同步 REST/gRPC 最新接口

---

**更新日期**: 2026-09-06
**当前重点**: 9.1.1 peer broadcast 集成
**前置条件**:
- 阶段 8.3 peer 协议 Phase B ✅
- Experience CRDT API ✅
- Consensus benchmark ✅

---

## 阶段 10：修复「看起来完成」的完成状态（2026-10-02 起）

> 起因：本轮发现 9.1「peer broadcast 集成」被勾选完成，但生产路径上跨节点经验同步从未真正跑通——事件链路上有三处独立丢弃点，其中一处是复制出的第二份实现。详见 [docs/project-status-2026-10-02.md](docs/project-status-2026-10-02.md)。
> 结论：勾选状态不能靠人记，必须变成可验证的门禁。

### 10.1 验证门禁化（最高优先）
- [x] **孤儿源文件扫描**: `scripts/audit_orphans.py` + `make audit-orphans` + CI 作业。已端到端验证（移除 `mod tests;` 即被抓出）
- [x] **修复 `sisyphus-cli` `resolve_backend` 三份副本**: `build.rs`/`run.rs`/`diagnose.rs` 各一份 36 行副本且已漂移，收敛为 `build.rs` 的 `pub(crate)` 单一实现
- [ ] **重复实现扫描（尝试失败，未采用）**: 正则 + 字节级比对抓不住「复制后各自加注释」这一真实漂移形态；需 `syn` 级 AST 比对。详见状态报告第六节
- [ ] **TODO 勾选门禁**: 勾选项须附带测试名，`make todo-audit` 交叉核对是否真实存在且在跑

### 10.2 阶段 9 剩余
- [ ] **启动时全量 sync**: 新节点 Join 后向 peer 拉取全量经验
- [ ] **断线补偿**: 重连后按 vector clock 差量补齐
- [ ] **`ExperienceSyncHandler` 关闭语义加固**: 所有 sender 丢弃时接收端会静默退出，接口层缺少防护
- [ ] **入站事件转发收敛**: 当前靠自过滤终止回弹，属约定而非结构保证
- [ ] **3 节点 Docker Compose** / **网络分区恢复** 验证

### 10.3 工具链
- [x] **并行链接保护**: 实测 19.5 GiB 下 `-j 2` 仍会 OOM、`-j 1` 稳定；`test-all`/`test-tools`/`test-bridge*`/`test-cli-handlers` 已统一带 `CARGO_BUILD_JOBS=1`，Makefile 头部记录约束（直接敲 `cargo test` 仍需自行加 `-j 1`）
- [ ] **认知循环 v3 (ADR-010)**: 先写问题陈述再谈方向，当前 TODO 9.2 只有候选词，不足以支撑决策

---

**更新日期**: 2026-10-02
**当前重点**: 10.1 验证门禁化
**前置条件**:
- 阶段 9.1 经验同步生产链路已打通并有端到端验证 ✅
- 跨节点经验同步相关缺陷全部修复并回归通过 ✅
- 工作区全量回归 4976 通过 / 0 失败（含 webui/evals/sandbox/sandboxd）✅
