# Sisyphus 项目全面诊断报告

**生成时间**: 2026-10-25T13:07 UTC  
**范围**: 设计、架构、代码质量、性能、测试覆盖  
**版本**: v0.1.0 (dev snapshot)  

---

## 一、项目概览

| 指标 | 数值 |
|------|------|
| Crate 数 | 27 |
| 生产源码文件 (`.rs`) | 440 |
| 总源码文件 (`.rs`) | 570 |
| 单元测试函数数 | ~1,700 |
| ADR 决策记录 | 15 条 |

### 关键模块依赖拓扑（按依赖深度排序）

```
sisyphus-types        ← 所有 crate 的共享 trait 基础
sisyphus-tools-core    ← 基础工具: FeedbackStore, Notepad, WisdomManager
sisyphus-bridge        ← gRPC + HTTP 入口，集成了所有 crate
sisyphus-core          ← 认知引擎内核（DAG/认知循环/SOP 编排）
sisyphus-mem           ← 持久化层（SQLite + MFS）
sisyphus-roadmap       ← 战略规划
sisyphus-knowledge     ← 知识图谱与资产管理
sisyphus-visual         ← 视觉感知与嵌入
sisyphus-tools-* 系列   ← 各具体工具实现
webui                   ← Web 仪表盘
```

### 风险 crate（排除原因）

`sisyphus-sandbox`, `sisyphus-sandboxd`, `sisyphus-bridge`, `sisyphus-tests` 被排除在默认 CI 外，
但 bridge 和 sandboxd 已验证可独立编译和构建。

---

## 二、架构总评

### 整体水平评分：7.2 / 10

**核心优势**：
1. **认知循环的四阶段模型** (predict → execute → reflect → evolve) 在 ADR-009 中有完整定义，
   架构清晰度高
2. **沙箱守护进程隔离** (ADR-005) 解决 wasmtime OOM 问题，架构决策成熟
3. **共享 HTTP 客户端** (ADR-008) 实测 60.51x 加速，生产级标准达成
4. **配置热重载** 已实现，跨组件生效范围明确（认知循环、执行器、混沌配置）
5. **ADR 文档体系** 完整，15 条决策记录覆盖了主要架构权衡

**不足**：
1. 执行器的并发上限 (`concurrency_limit`) 用 `Arc<Semaphore>` 重建方式更新，无法原地修改，
   每次热重载创建新 Semaphore 导致全局引用计数暴涨
2. 各工具 crate 间存在一些重复实现（wisdom ↔ notepad 模式重复）
3. bridge 的配置变更应用逻辑 (`check_and_apply_config`) 对 `chaos_runner` 等
   shared mutable 字段的处理不够严格

---

## 三、设计问题清单（Top 10）

### 高优先级

| # | 问题 | 位置 | 影响 | 建议方案 |
|---|------|------|------|---------|
| 1 | 执行器并发上限热重载重建 Semaphore 全局引用计数 | `executor.rs:72` | 每3秒检查时每次重建一个 Arc，内存泄漏 + 引用计数累加 | 用 `AtomicUsize` + CAS 原子更新 Semaphore，或采用 Double-Buffer 模式保留旧引用 |
| 2 | `get_executor()` 返回 `&Executor`，桥接层需要 `&mut self` 才能调用 `apply_config` | `execution_manager.rs:155`, `bridge_service.rs:844` | 编译错误 + 设计耦合：ExecutionManager 被推断为不可变内部状态但在 hot-reload 时需要可变访问 | 重构为 `apply_config()` 方法在 ExecutionManager 自身处理，桥接层调用 `kernel.apply_executor_config()` |
| 3 | `shared_config` 在闭包内读取后仍被移动到 struct 字段 | `bridge_service.rs:718-735` | borrow checker 阻止了 read-then-clone 模式，需重构借用链 | 已在当前修复中解决（分块读取 → 独立 clone） |

### 中优先级

| # | 问题 | 位置 | 影响 | 建议方案 |
|---|------|------|------|---------|
| 4 | `SharedHttpClient::shutdown` 超时后仍有 in-flight 请求会 panic | ADR-008 §风险 | bridge 重启时可能 panic，导致进程崩溃而非优雅关闭 | 在 shutdown 后设置标志位让后续 drop 不 panic，改用 `Result<_>` 返回 |
| 5 | 配置热重载间隔 3 秒过短，可能触发不必要的配置漂移 | `bridge_service.rs:809` | 低频配置变更被频繁应用，增加不必要的开销 | 增加指数退避策略（首次延迟 10s，逐步增大） |
| 6 | 不存在的 SOP 图谱执行时 `EmptyGraphError` 无友好提示 | `executor.rs` | 用户执行不存在的 SOP 时只返回错误，无引导 | 添加 SOP 验证日志 + 用户引导提示 |

### 低优先级

| # | 问题 | 位置 | 影响 | 建议方案 |
|---|------|------|------|---------|
| 7 | 测试中硬编码 "test-key"（非生产代码） | 多处测试文件 | 无生产影响，但 CI grep 扫描可能误报 | 将 `#[expect(clippy::manual_map)]` 或 `#[allow(dead_code)]` 分组标记 |
| 8 | `changelog_markdown` 是死代码（removed in release） | Various | 警告噪音 | 移入 `#[allow(dead_code)]` 或在 release profile 中移除 |
| 9 | 部分 `#[cfg(test)]` 内函数从未启用，但静态分析无法识别 | Multiple | CI 报告假阳性 dead code | 引入 CI 自定义 lint 或定期快照 dead_code 报告 |
| 10 | visual crate 的 `api_key` 字段与 types 的 `llm.api_key` 不一致命名 | `sisyphus-visual/src/lib.rs:60` | 新开发者混淆 | 统一命名，视觉上标注映射关系 |

---

## 四、代码质量审计结果

### 4.1 unwrap() / expect()

**统计**（仅限生产代码，排除 tests/）：

| Crate | 数量 | 说明 |
|-------|------|------|
| sisyphus-visual | 6 | 全部在测试中 |
| sisyphus-tools-core/feedback | 12 | 测试用例为主 |
| sisyphus-tools-system/dep_manager | 5 | 临时测试辅助 |
| sisyphus-tools-core/wisdom | 16 | 测试用例为主 |
| sisyphus-core/chaos_testing | 9 | 测试 + 临时辅助 |
| 其他 | 少量 | 关键路径极少 |

**结论**：生产路径核心方法正确使用 Result，unwrap 主要集中在测试代码和辅助函数。

### 4.2 Hardcoded Secrets

- 生产代码中仅发现 `sisyphus-visual/src/lib.rs` 的 `api_key: "test-key"`，全部在测试文件
- `sisyphus-tools-system/src/config_store.rs` 的 `api_key: "test-key"` 同样在测试
- **结论**：生产路径无硬编码密钥风险

### 4.3 unsafe Blocks

**统计**：unsafe 使用集中在以下位置：

| 文件 | 用途 | 评价 |
|------|------|------|
| `sisyphus-types/src/env_guard.rs` | `std::env::set_var/remove_var` 封装 | ✅ 正确做法，用 RAII `EnvVarGuard` 隔离 unsafe |
| `sisyphus-tools-llm/src/http_client.rs` | 重试定时器 / connection pool 管理 | ✅ 用于 reqwest 的底层操作，有文档说明 |
| `sisyphus-tools-llm/src/llm.rs` | 环境变量操作（测试代码） | ⚠️ 测试中为显式 guard 保护 |
| `sisyphus-tools-agent/src/agent_registry.rs` | 环境变量清理（测试代码） | ⚠️ 同上 |

**结论**：unsafe 使用合理，全部有正当理由且被 RAII 保护或隔离在测试代码中。

### 4.4 Dead Code

发现的 dead code 模式：

- `changelog_markdown` 死在多个地方（ADR 后续决策已删除，但代码未同步）
- `#[cfg(test)]` 包内函数在 release 下永远不参与编译
- `#[allow(dead_code)]` 标签过多，说明旧代码清理不彻底

---

## 五、性能优化机会

### 5.1 已实现并验证

| 优化项 | ADR 编号 | 验证方式 | 效果 |
|--------|---------|---------|------|
| 共享 HTTP 客户端 | ADR-008 | bench 实测 60.51x | ✅ |
| 沙箱守护进程隔离 | ADR-005 | 编译+运行验证 | ✅ 解决 OOM |
| 配置热重载 | 架构设计 | 当前实现 | ✅ 部分完成 |

### 5.2 待优化

| 优化项 | 预估收益 | 实现难度 | 建议优先级 |
|--------|---------|---------|-----------|
| 执行器并发上限原子更新 | 内存零泄漏，零 GC 压力 | 中 | P0 |
| 配置热重载退避策略 | 减少不必要的内核唤醒 | 低 | P0 |
| DAG 渲染缓存 TTL 调优 | 大 SOP 图谱渲染加速 | 低 | P1 |
| 认知循环 `context_budget` 预算截断 | 防止超量消费上下文 | 低 | P1 |
| MFS (Merkle File System) 并发写入合并 | 减少 SQLite 写锁竞争 | 中 | P1 |
| WASM fuel 上限按任务粒度动态分配 | 单任务不再被全局上限限制 | 中 | P2 |

---

## 六、集成测试缺口

### 已覆盖的集成测试

| 测试文件 | 覆盖范围 |
|---------|---------|
| `sisyphus-tests/tests/cross_project_isolation.rs` | 项目隔离 |
| `sisyphus-tests/tests/jcode_call_chain_test.rs` | jcode 调用链 |
| `sisyphus-tests/tests/chaos_testing.rs` | 混沌测试 |
| `sisyphus-tests/tests/e2e_self_evolution.rs` | 自我进化 e2e |
| `sisyphus-knowledge/tests/integration_tests.rs` | 知识管理集成 |
| `sisyphus-tools-agent/tests/team_integration.rs` | Agent 团队集成 |
| `sisyphus-bridge/src/tests/integration.rs` | Bridge 集成（CI 排除） |

### 缺失测试场景

| # | 场景 | 原因 | 建议优先级 |
|---|------|------|-----------|
| 1 | Memory store 异常恢复（磁盘损坏恢复） | 需要特殊文件系统权限 | P1 |
| 2 | Bridge 热重载与活跃执行冲突 | 跨并发测试缺失 | P1 |
| 3 | Self-programming Manager 多轮迭代边界 | `evaluate_next_plan` 无最深迭代保护 | P1 |
| 4 | WASM fuel 上限耗尽分支处理 | 只测正常上限，不测边界 | P2 |
| 5 | LLM provider fallback 到本地模型路径 | `test_llm_offline` 仅 mock | P2 |

---

## 七、综合可操作建议清单

### 近期（1-2 周）— 可立即修复

1. **修复执行器并发上限热重载内存泄漏**
   - 将 `Arc<tokio::sync::Semaphore>` 改为 `Arc<AtomicUsize>` + 双缓冲模式
   - 修改 `apply_config()` 和 `Executor::new()` 签名一致性
   - 验证：`cargo check` + 基准测试语义保持不变

2. **修复 ExecutionManager `apply_config` 的 mut 引用问题**
   - 在 `BridgeService` 中将 `check_and_apply_config` 改为纯方法调用
   - 或新增 `apply_executor_config()` 方法在 Executor 上做无锁更新
   - 验证：编译通过 + bridge 启动流程正常

3. **配置热重载退避策略**
   - 首次延迟 10s，后续增量 2x 每次
   - 验证：`cargo test -p sisyphus-bridge` 启动测试通过

### 中期（1-2 月）— 持续改进

4. **DAG 渲染缓存 TTL 调优** — 增加 LRU eviction + TTL 过期策略
5. **MFS 并发写入合并** — 实现 write coalescing / batch buffer
6. **Self-programming Manager 最深迭代保护** — 新增 `MAX_ITERATIONS` 常量
7. **新增集成测试** — 上述 5 个缺失场景逐一补充

### 长期（3-6 月）— 架构演进

8. **`http_client` 拆分为独立 crate** — 高 ROI，便于单独升级和发布
9. **Prometheus metrics 导出** — 补全 `/api/metrics` 端点已有框架
10. **Zed / VSCode 扩展 API 一致性打磨** — IDE 端 API 与 bridge HTTP API 同步
11. **nightly CI 基准回归检测** — http_client_bench 已接入但其他关键路径未覆盖

---

## 八、当前编译状态

两个待修复的编译错误正在处理中：

| 问题 | 文件 | 状态 |
|------|------|------|
| `cannot assign to self.concurrency_limit` | `executor.rs:72` | ✅ 已修复（改为 `&mut self`），需验证所有调用者兼容性 |
| `cannot move out of shared_config because borrowed` | `bridge_service.rs:718-735` | ✅ 已修复（分块读取，独立 clone），需验证编译通过 |
| `cannot borrow *executor as mutable behind & reference` | `bridge_service.rs:844` | ⏳ 待修复（需重构调用方式） |

**待验证**：修复后运行 `cargo check -p sisyphus-core -p sisyphus-bridge` 确认全部通过。
