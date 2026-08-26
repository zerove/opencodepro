# Sisyphus 架构评审（2026-08-26）

> 范围：全工作区约 9.5 万行 Rust，30 个成员 crate。方法：文档比对（README / book / ADR / ROADMAP / EVOLUTION_PLAN）+ 源码逐层审查（core 认知循环、bridge 链路、mem 存储层、tools 层、CI 与测试）。所有论断附文件级证据。

## 一、项目目标理解

三级跃迁：**目标执行器** → **自主项目架构师**（ROADMAP.md 已达成）→ **工业级通用认知引擎**（EVOLUTION_PLAN 进行中）。本质是"以 LLM 为认知组件、以 DAG 为结构骨架"的自适应执行引擎，核心创新在于将反馈控制回路显式化为四阶段循环（predict → execute → reflect → evolve），并把 SOP 图谱作为一等可变对象在运行时演化。

## 二、做得好的地方

| 方面 | 评价 |
|------|------|
| 分层纪律 | types 作依赖根零下行依赖；core 只组合不实现工具；依赖流向单向 |
| trait 边界 | Simulator/Reflector/SOPEvolver/ExperienceBank/ToolExecutor 全部异步 trait 化 |
| 演进门禁 | evolve 结果须经 verify_evolution 模拟验证优于当前图才采纳（cognitive_loop.rs） |
| 沙箱架构 | 嵌入式/sandboxd 双模式 + ADR 005 记录 OOM 权衡 |
| 工程基建 | cargo-deny 供应链审计、per-crate CI 隔离防 OOM、MfsStore 两级缓存（L1 LRU + L2 SQLite WAL） |

## 三、问题清单（均经源码二次核实）

### A. 架构层（高）

- **A1. v2 组件半接线**：`ParallelBranchPredictor::evaluate_branches` 仅被 benches 引用，主认知循环从未调用。AdaptiveDepthController 与 ComplexityAnalyzer 两套深度预算取 min 叠加，调参面翻倍。
- **A2. 共享状态跨执行污染（已证实的并发路径）**：gRPC `ExecuteSop` → `run_execution` → `kernel.execute_goal` → `execute_with_cognition` → `run_cognitive_loop`，tonic 天然并发且无互斥；更强的是 `kernel/execution.rs:527,583` 的 `execute_composite` 用 `JoinSet.spawn` **主动并行**多个循环实例，每个都写共享 `self.config`（cognitive_loop.rs:88 写回 max_iterations/convergence_threshold）。`depth_controller.reset()` 全局 0 个调用点，置信度历史跨执行累积导致误判收敛。`active_executions` 以 sop_id 为键互写。
- **A3. sisyphus-api 孤儿化**：无下游依赖者，4 个端点与 bridge 内置 71 个 HTTP 端点重叠。
- **A4. 认知循环配置全链路断层（三次复核确认）**：`config.example.toml` 与 `book/src/configuration.md` 详细记载了 `[cognitive_loop]` 及 `[cognitive_loop.adaptive_depth]` 共 6 个可调参数与环境变量，但生产装配路径 `SisyphusKernel::new` 硬编码 `CognitiveLoopConfig::default()`（kernel/construction.rs:88），bridge 从不传递已解析的 `Config.cognitive_loop`；唯一的接线入口 `with_cognitive_loop_config` 在全仓库（含 ide/、evals/、webui/）零调用。且循环内动态深度计算会覆盖基线的 max_iterations/convergence_threshold（P0-1b 已局部化），因此**用户在配置文件或环境变量中设置的认知循环参数实际从不生效**。AdaptiveDepthController 同样以硬编码默认值构造（StrategicManager::new 中 `AdaptiveDepthController::default()`），未读取 `adaptive_depth` 配置段。

### B. 认知质量层（高）

- **B1. 伪嵌入客户端**：`DummyEmbeddingClient` 用文本哈希正弦生成伪向量，无语义区分度。装配逻辑 bridge_service.rs:188 仅当 `embedding_provider == "openai"` 才用真嵌入，其余一切取值（含默认）静默降级 Dummy；另 :480 处无条件硬编码。EVOLUTION_PLAN 1.2 标 ✅ 但验收标准（召回率对比）无证据。
- **B2. 演进信号为字符串匹配**：reflect.rs 中 `suggested_improvements.iter().any(|s| s.contains("retry logic"))` 等，反思→演进闭环建立在 LLM 英文子串匹配上。
- **B3. LLM 审计超时 2 秒**（✅ 已实施统一旋钮；后补：kernel 构造时曾用默认参数重建 LlmLogicalSimulator 架空 bridge 注入的配置超时实例，已改为复用注入的 intel 字段，`with_cognitive_loop_config` 同时修正为委托 `apply_cognitive_loop_config` 完成控制器重配）：全系统 5 处硬编码 2s（critique_reflection、LlmReflector×2、LlmEvolver、LlmLogicalSimulator）统一收敛为单一配置 `cognitive_loop.llm_stage_timeout_secs`（默认 30s；0 禁用对应阶段 LLM 调用）；三个 intel 组件经构造函数注入（`with_timeout`），旧 `new` 签名保留 2s 默认供测试零破坏。bridge 生产构造点接线该配置。micro_reflector 与 gap_analyzer 的 2s 为每节点热路径的显式设计（有注释论证），保持不变。回归测试：批判与反思阶段的慢 LLM 切断降级、预算内透传、禁用语义。重试退避仍未实施。
- **B4. 可观测数据失真**：CogMap 节点偏差恒 0（源码自认注释），节点状态靠 `contains("Error")` 嗅探。

### C. 工程质量层（中）

> 勘误（2026-08-26 二次复核）：初版按文件名过滤统计得"约 886 处"，该数字将文件尾部内联 `#[cfg(test)]` 模块误计入生产代码。精确截断内联测试模块后，**真实生产代码仅约 39 处**，其中 16 处在 benches、7 处在 build.rs、6 处在 sisyphus-tests crate；核心运行时路径合计约 10 处（sdk 3、config_store 2、momus 2、observability 2、tauri 1）。项目自身的错误处理纪律实际执行良好，此问题降级为低优先级清理项。

- 错误类型三风格并存：`IntelError` 枚举、裸 `String`（simulation.rs 多处）、`anyhow::Result`（MfsStore），跨层语义丢失。
- `UserMemoryStore.recall` 先取全局 top_k×10 再按会话前缀过滤，多会话下目标记录可能不在截断窗口内。
- reset() 结论复核确认：`AnomalyDetector::reset` 与 `AdaptiveDepthController::reset` 定义存在（注释明确写"为新一轮执行周期重置"），core 全量搜索 `.reset()` 调用点为 0，A2 成立。

### D. 验证能力（中高）

28 个 crate 对应的评测资产仅 evals/runner.rs（404 行）。EVOLUTION_PLAN 2.1 "拦截 ≥20% 错误 SOP" 类验收标准不可度量。

## 四、优化方案

### P0 正确性修复（1-2 周）
1. 循环参数局部化传参，禁止写回共享 `RwLock<CognitiveLoopConfig>`；AdaptiveDepthController 改 per-run 实例或进入循环前 reset()（✅ 已实施，见 265b958 + dbc2f7a）
1b. 接线认知循环配置（✅ 已实施，e3f56bb）：bridge 在 kernel 构造后调用新增的 `StrategicManager::apply_cognitive_loop_config(&self)`；控制器两个可调字段（target_confidence/min_improvement）改为 RwLock 内部可变以支持共享 Arc 下的运行时重配。映射关系：`convergence_threshold → target_confidence = 1 - threshold`（clamp 到 [0.5, 1.0]），`min_improvement` 直传。遗留缺口（如实记录）：动态深度计算的输出（ComplexityAnalyzer 固定档位 3/10/20/30 与控制器初始预算取 min）仍**完全忽略**基线 `config.max_iterations`——用户设置的最大迭代上限即使接线后也不生效；`observe_iteration` 的硬上限仅在"平台期提前停止"分支且带 `convergence_threshold*1.5` 门控时被消费，不是无条件硬顶。已修复（275a435）：`final_max` 现与基线取 min，语义定为「自适应只能在用户预算内减少迭代、不得突破上限」；并提取纯函数 `effective_max_iterations` 补充回归测试（19ff3a4：上限优先、自适应只减不增、零上限禁用循环、运行时重配立即生效、重置防跨运行污染，共 5 个测试）
2. 错误类型统一（维持）：simulation.rs 的 String 错误收敛到 IntelError；unwrap 清理降为低优先级（真实生产路径仅约 10 处，见 C 层勘误）
3. NodeOutput 增加 status 字段，消灭字符串嗅探（✅ 已实施：`NodeOutput::explicit_status()` 按工具生态既有约定依次探测 `status` 字符串/`success` 布尔/`exit_code` 整数；`indicates_failure()` 显式信号优先、旧文本嗅探仅作无信号时的兜底。supervisor 快速通道与 CogMap 状态判定两处生产嗅探点已切换，5 个回归测试覆盖优先级与误报场景——显式 status=ok 而输出文本含 "Error" 时不再误判）

### P1 架构收敛（2-4 周）
4. 决断 v1/v2 关系：接入 ParallelBranchPredictor 或删除并修订 ADR 005；自适应深度保留 ComplexityAnalyzer 初始预算 + AdaptiveDepthController 运行时终止，去掉双重计算
5. 演进信号枚举化：`enum ImprovementKind { Retry{max}, FuelIncrease{factor}, NodeSplit, Backoff }`，ReflectionReport.suggestions 改为 `Vec<(NodeId, ImprovementKind)>`
6. sisyphus-api 合并入 bridge 或转为纯 OpenAPI 类型定义 crate

### P2 认知能力提升（1-2 月）
7. 真实嵌入接入：配置缺失时显式降级并告警（✅ 第一步已实施：非 openai provider 时启动日志显式 warn 说明伪嵌入无语义质量与检索退化，DummyEmbeddingClient 补充警示文档；Jaccard 回退实现与召回率基准待实施）
8. LLM 调用分级治理：critique 阶段超时可配置已落地（30s 默认 + 禁用语义，见 B3）；reflect/evolve 分级、指数退避与 token 预算联动仍待实施
9. 偏差真值采集：复用 Executor 已有 fuel/duration 数据回填 CogMap 与 reflect 阶段

### P3 生态与验证（持续）
10. 构建 ≥50 个带标准答案的 golden SOP 库，nightly 跑通过率/成本/迭代数趋势
11. 技能市场对等协议加签名校验与速率限制
12. DAG 执行器引入属性测试（随机 DAG 验证拓扑不变量）

## 五、总结

架构审美在线、工程习惯良好（生产代码错误处理纪律经精确复核确认良好），但"宣称能力 > 验证能力"。关键断层集中在认知闭环节点（反思信号传递、经验检索、LLM 调用）的"演示可用、生产失效"，以及并发执行下的共享状态隔离缺失。优先序：P0 并发正确性 → P1 信号结构化 → P3 评测体系兜底其余演进。
