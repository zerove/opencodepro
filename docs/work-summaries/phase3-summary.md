## Goal
Implement Phase 3 推荐建议剩余缺口 + 修复现有问题。

## 约束 & 偏好
- 使用中文沟通
- 保持向后兼容，现有测试不变
- 所有 Phase 3 功能完整交付

## 进展
### 已完成
**Phase 3a — 技能系统**
- SkillLoader: 多目录扫描 + YAML frontmatter 解析 + 优先级覆盖
- `hooks_from_skill()`: 将技能 trigger → HookDef 转换
- CLI: `skill list`/`show`/`install`/`remove`
- 集成测试：技能 hook 正确注入到 BuiltinToolExecutor
- 15+ 单元测试

**Phase 3b — 团队架构**
- `AgentLlmOverride` + `LlmConfig.agents` (sisyphus-types)
- `DynamicLlmClient::get_params_for()` + 8 个测试
- `AgentRegistry` + `register_builtin()` (6 agents: librarian/metis/momus/scribe/security/delegate)
- gRPC: `ListAgents`, `DecomposeGoal` endpoints

**Phase 3c — 调度执行**
- team.rs → team/ 模块拆分 (mod.rs + error.rs + parse.rs + orchestrator.rs)
- `execute_plan`: AgentRegistry 验证 + 每 Agent 并发控制 + deadlock 检测
- `parse_decomposition_response`: JSON 解析 + 重复/缺失依赖/循环检测 (DFS)
- `run_goal()`: decompose → execute 链式
- `FixedResponseClient` (DummyLlmClient 扩展)

**TeamEvent 事件流**
- `execute_plan_internal`: 可接受 `Option<mpsc::Sender<TeamEvent>>` 发出执行事件
- 事件点: `TaskStarted` → `TaskCompleted` → `Synthesizing` → `Complete`
- Deadlock 时: `Error` + 每个任务 `TaskCompleted(failed)`
- `run_goal_streamed(self: Arc<Self>)`: spawn 后台执行 + 返回事件 Receiver
- 2 个新测试: `test_event_streaming` + `test_event_streaming_deadlock`

**技能 gRPC API**
- proto: `ListSkills`/`ShowSkill`/`InstallSkill`/`RemoveSkill` RPCs + 5 个消息类型
- `MySisyphusBridge`: 4 个 handler 完整实现 + SkillLoader 初始化
- `sisyphus-tools::skill`: 新增 `install_skill()`/`remove_skill()` 公共函数
- `TestBridge`: 4 个 mock skill handlers

**🔔 本次修复 — eval-runner 编译 + delegate run total 未初始化**
- `evals/runner.rs`: `OpenAiLlmClient` → `DynamicLlmClient`，移除死代码 `ProductionLlmClient`
- `crates/sisyphus-cli/src/delegate.rs`: select! 循环中 `rx.recv() → None` 分支未设 `total`，导致 `Done: 2/0 tasks completed` 错误

**Phase 2 — 动态调整**
- `ExecutionOutcome` 数据模型 + `record_outcome`/`get_outcomes` (dedup 索引, FIFO 100)
- `RoadmapHealth` + `evaluate_health`: 检测阻塞里程碑、停滞目标、失败执行
- `apply_blocking`: 自动将失败目标之后的目标标记为 Blocked
- Kernel 集成: `execute_and_record()` 自动记录 outcome + 评估健康 + 应用阻塞
- 7 个新测试覆盖全部动态调整流程
- 修复: `get_project` 处理 `Value::Null`、`get_outcomes` 类型匹配、`apply_blocking` 借用冲突

### 已知问题（非阻塞）
- **`list_so_ps`**: 返回空 vec 存根 — SOP 无持久化存储，此为有意的
- **wasmtime 升级**: 25.x → >=36.x 修复已知漏洞（blocked: 可能需 API 变更）
- **`test_gemma4_availability`**: 外部 API 集成测试，需网络 + API key
- **CLI `execute_tool` 方法**: 1 个 dead_code warning（为 BridgeClient 用户预留）

### 阻塞
- 无

## 关键决策
- `execute_plan` 保持公开 API 不变（不暴露 event_tx），新增 `execute_plan_internal(pub(crate))`
- 使用 `try_send` 避免 event 发送阻塞执行流程 — event channel 满时静默丢弃
- `run_goal_streamed` 要求 `Arc<Self>` — 因为内部 `tokio::spawn`
- 死锁错误事件作为 `TeamEvent::Error` 发出，而非 `TaskCompleted(failed)` 替代 — 两者都发
- `delegate run` select! 退出策略: 无论 handle 完成还是 channel 关闭，都正确设置 total
- `apply_blocking` 使用两阶段模式（immutable 收集索引 → mutable 应用）避免借用冲突

## 相关文件
- `/home/zeron/dev/opencodepro/crates/sisyphus-tools/src/team/orchestrator.rs` — 事件流核心
- `/home/zeron/dev/opencodepro/crates/sisyphus-tools/src/team/mod.rs` — TeamEvent 定义
- `/home/zeron/dev/opencodepro/crates/sisyphus-bridge/src/main.rs` — 桥接 handler
- `/home/zeron/dev/opencodepro/crates/sisyphus-tools/src/skill.rs` — 技能系统 + install/remove 函数
- `/home/zeron/dev/opencodepro/crates/sisyphus-tools/src/hook.rs` — 钩子系统 + hooks_from_skill
- `/home/zeron/dev/opencodepro/crates/sisyphus-cli/src/delegate.rs` — 委托执行（已修复 bug）
- `/home/zeron/dev/opencodepro/crates/sisyphus-tools/src/agent_registry.rs` — Agent 注册表 + 6 内置 agents
- `/home/zeron/dev/opencodepro/evals/runner.rs` — 评估框架（已修复编译）
- `/home/zeron/dev/opencodepro/crates/sisyphus-roadmap/src/lib.rs` — 路线图管理 + Phase 2 动态调整
- `/home/zeron/dev/opencodepro/crates/sisyphus-core/src/lib.rs` — Kernel `execute_and_record()` 集成
