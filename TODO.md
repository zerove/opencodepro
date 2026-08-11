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

---
**更新日期**: 2026-08-11
**当前重点**: 单机全功能 — 可解释性深度闭环（追踪可视化 → SQL 持久化 → 历史检索 → 跨会话健康摘要），下一项: 阶段 5 剩余能力 (待评估，或按需深化多智能体/认知循环)