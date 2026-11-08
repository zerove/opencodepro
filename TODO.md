# Sisyphus 后续开发计划 (TODO.md)

> **当前状态报告**: [docs/project-status-2026-10-04.md](docs/project-status-2026-10-04.md)（最新快照。多语言测试守卫的 6 个缺陷全部修复并有真实工具链端到端验证：Go 多包部分构建失败曾被漏判、Python 全部通过曾报 `passed=0`、Node 混合结果曾报 `passed=2 failed=0`、vitest 套件加载失败守卫因只读 stdout 而从不触发、以及收尾时才发现的 Java/Maven 同类缺陷（`[INFO]` 前缀 + 千分位逗号使解析永不命中，且 `test()` 丢弃 `success` 导致编译失败被当成全通过）。根因 `sisyphus-cli/src/build.rs` 的 `if r.failed == 0 { "ok" }` 已改为 `test_status_label()`，区分 `ok` / `FAILED` / `no tests ran`，配 9 条单测。真实工具链 e2e 从 11 条增至 13 条，并**已接入 CI**（新增 `toolchain-e2e` job）；顺带修复了长期失败的 `audit-sources` job。全工作区回归 4762 通过 / 0 失败。历史快照见 [docs/project-status-2026-10-02.md](docs/project-status-2026-10-02.md)。）

## 已完成 (Done)

> **勾选约定（2026-11-04 起）**：`- [x]` 表示「已验证」，必须附可验证证据——
> `path/to/file.rs::test_name`（测试须真实存在、带测试属性、未被 `#[ignore]`，
> 或已由 `--ignored` job 执行）、`make <target>`、`scripts/x.py`。
> `make todo-audit` 与 CI 的 `audit-sources` job 会交叉核对。确无自动化测试时写
> `（无测试：理由）`，不要让勾选本身充当证据。历史勾选项登记在
> `scripts/todo_baseline.txt`（ratchet，只减不增）。
> 测试名约定：引用的测试 fn 须 `fn test_*` 前缀；文件级回归测试允许在 `tests/`
> 目录下的文件以任意 fn 名全路径引用。

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
- [x] **多语言测试守卫的 6 个解析器缺陷修复** (2026-10-04)
    - Go 多包部分构建失败漏判（改用 `[build failed]` 标记）
    - Python 全部通过报 `passed=0`（`strip_suffix(" passed")` 对 `1 passed in 0.36s` 永不命中）
    - Node 混合结果报 `passed=2 failed=0`（整行数字被当成 passed 的值）
    - vitest 套件加载失败守卫从不触发（标记在 stderr 且带 ANSI）
    - 失败用例名带 ANSI 转义码（解析前未去色）
    - **Java/Maven 同类缺陷**（收尾时用真实 `mvn` 才发现）：`[INFO]` 前缀使 `starts_with` 永不命中、数字带千分位逗号、`test()` 丢弃 `success` 使编译失败被当成全通过
- [x] **测试判定式根治** (2026-10-04): `sisyphus-cli/src/build.rs` 的 `if r.failed == 0 { "ok" }` → `test_status_label()`，区分 `ok` / `FAILED` / `no tests ran`，9 条单测 + 真实 CLI 四路径验证（空工程 / 通过 / 失败 / frontend）
    - 零用例提示语按 `backend.name()` 分流：frontend 说「没有测试步骤」而不是「过滤器没命中」（实测发现第一版统一提示语对 HTML/CSS 是错误建议）
- [x] **真实工具链 e2e** (2026-10-04): `language_backend_guards.rs` 11 → 13 条（新增 Maven 编译失败 / 全通过），并**接入 CI**（新增 `toolchain-e2e` job，装 go/python/pytest/JDK/Maven）
- [x] **修复长期失败的 `audit-sources` CI job** (2026-10-04): `scripts/audit_ignored.py` 退出码 1 → 0
    - `blank_comments()` 不认 Rust raw string（`r#"..."#`），把探针源码里模拟的 `#[ignore]` 当成真实忽略点
    - `ACCEPTED` 白名单太窄，`requires real python3 + pytest` / `requires real mvn + jdk` 这类诚实理由反被判非法
    - 双向实测：注入 `#[ignore = "测试当前失败"]` 仍被判非法（未放松门禁）
- [x] **交付 rustc/链接器 SIGABRT 取证工具** (2026-10-04): `make rustc-abort-forensics`，实跑通过
    - 根因：`ulimit -c = 0`，崩溃不留现场——之前“无法取证”并非日志被覆盖，而是从未记录
    - 轮询强制 `touch lib.rs` 重编（否则 cargo 命中缓存，循环无意义）；输出落盘到 `$JCODE_SCRATCH_DIR/forensics/`
- [x] **SIGABRT 成因已查明** (2026-10-04 晚): 崩在**链接期 rust-lld**，抛 `EAGAIN «Resource temporarily unavailable»`
    - 真因：本机 `vm.overcommit_memory=2`（严格模式）+ `ratio=80`，`CommitLimit ≈ RAM×80%`（21.1 GiB）
    - 实测：崩溃当轮 `Committed_AS` 冲到 **92%**，成功轮次峰值仅 88%——相关性直接可见
    - **关键教训**：排查须看 `Committed_AS` 而非 `free available`（后者有 10 GiB 会误导）
    - `-j 1` 不是无效：它降低并行链接数，确实降低提交峰值，只是无法消除单链接器占用
    - 根治三选一（切 mold / `CARGO_PROFILE_DEV_DEBUG=0` / 调 overcommit_ratio）：
      **实测过 mold 与 debug=0 两者**；`overcommit_ratio` 仅**评估**未实测（需 root），
      结论见下方「SIGABRT 根治手段」条（mold 无效、debug=0 仅降 4.7%）
    - **不改系统级 overcommit 设置**（需 root，影响该机器所有用户）
- [x] **嵌套 cargo 撞同一堵墙（第一轮修错，第二轮才修对）** (2026-10-04)
    - 第一轮修错：`cargo_raw` 注入 `CARGO_BUILD_JOBS=1` 只管**单个** cargo 内部并行度，
      管不到「同时有几个嵌套 cargo」；7 条用例被 libtest 并行调度，**守卫层错**
    - 推翻它的证据：`make test-all` 真实退出码 **2**（不是 0），`rust_backend_test_e2e` 2 条失败，
      错误为 `LLVM ERROR: out of memory` + `signal 6 [Aborted]`
    - 第二轮修复：测试文件内**跨用例全局 `Mutex` 闸门**，并发嵌套 cargo 限为 1
      - `unwrap_or_else(|e| e.into_inner())` 防一个用例 panic 后**二次毒化**后续全部用例
    - 对照压测（`--test-threads=8`）：修复前 10 轮失败 1，修复后 26 轮 0 失败
    - **验收补强**（原仅有单次绿灯，概率性故障不足以据此宣称修复）：
      - 真实验收路径连续 3 轮完整 `make test-all`：`MAKE_EXIT=0`、`4763/0`、`abort_hits=0`
      - CI 覆盖已证实：`test-light` 的 `--workspace` 隐式含该 target
        （`cargo test --workspace --no-run --message-format=json` 里可见 `rust_backend_test_e2e`）
        ——**我曾用 grep 误判为「CI 完全没覆盖」，是自己推翻的错误结论**
      - 闸门覆盖 7/7 用例无缺口；`serialized` 内无嵌套调用故非重入 Mutex 不会死锁
      - 探针坑：验证 poison 恢复时第一版在持锁状态下重入 → **探针自死锁**，
        非闸门缺陷；插桩后按忠实场景重测才得出有效结论
      - **后续补测才发现漏了第二个进程**：`src/build/rust_tests.rs` 里 7 个单元测试
        （`test_check_on_*` / `test_fmt_check_*` / `test_build_valid` / `test_test_*`）
        同样拉起嵌套 cargo，与集成测试是**不同进程**，闸门互不约束
        → 真实代价：`make test-all` 跑出 `MAKE_EXIT=2`（`test_test_with_failures`
        因 rust-lld EAGAIN 失败）
      - **教训**：闸门加在了**文件级**，正确作用域是**所有会拉起嵌套 cargo 的测试进程**；
        「文件内无遗漏」≠「进程内无遗漏」，更≠「跨二进制无遗漏」
      - **诚实点**：回退修复做同条件 A/B 时**对照组也 12/12 全过**，
        说明隔离运行复现不出该故障，**修复必要性未被证明**。
        已证实的是「缺口真实存在且已造成过真实红灯」，不是「闸门已验证有效」
        —— 单 crate 压测对本故障没有检出力
    - 坑：注入用**子进程作用域**（`run_command_with_env`）而非 `std::env::set_var`（后者多线程下不健全）
    - 坑：`--test-threads=1` 能救是因为限制 cargo **个数**，不是 `CARGO_BUILD_JOBS` 起了作用（归因曾写反）
    - 坑：libtest **捕获通过用例的 stdout**，`println!` 默认根本不出现在日志里，别拿它当诊断手段
    - 教训：修复后必须跑**完整**负载并看**真实退出码**，只看「4763 passed / 0 failed」会漏掉 exit 2
    - 坑：文档里的输出样例必须标 ```text，否则被 rustdoc 当 Rust doctest 编译而失败（我自己踩的）
    - 教训：单跑该测试**无法**复现，对照实验只有在原负载下才有意义
- [x] **SIGABRT 根治手段：三个候选全部实测完毕，均不能在仓库内根治** (2026-10-04)
    - 根因已定性：**提交内存（commit）额度耗尽**，不是真实 RAM 不足
      - `vm.overcommit_memory=2` + `overcommit_ratio=80` → `CommitLimit ≈ 21123 MiB`
      - lld 申请大块连续虚拟地址时，即便 available 还有 10 GiB 也会因**提交量超限**返回 `EAGAIN`
      - 这解释了「随机」表现：取决于当时 `buff/cache` 与其他进程的提交量
    - `CARGO_PROFILE_DEV_DEBUG=0`：实测降 **4.7%** 峰值，**不足以根治**
    - 切 `mold`（本机 2.40.4）：**无效**——`mmap failed: Cannot allocate memory`，
      与 rust-lld 的 `EAGAIN` 是**同一面墙**，且崩得**更早**（编依赖阶段就挂）
    - 关键认知：换链接器省的是 **RSS**，撞的是 **commit**，两者不是一回事，**思路本身错了**
    - `overcommit_ratio`：**需 root、改全机，仓库不做**
    - **结论**：仓库侧只能**收窄触发面**（`CARGO_BUILD_JOBS=1` + 嵌套 cargo 闸门），
      根治需机器所有者调高 `overcommit_ratio` 或加内存
    - 取证工具：`make rustc-abort-forensics`
- [ ] **链接器 A/B 的方法学教训（本轮踩坑，值得记住）** (2026-10-04)
    - **不要用机器级 `Committed_AS` 做单次对照**：本机 8 个 java 进程占 ~6.4 GiB，
      机器级指标波动主要来自它们而非链接器。实测那次 `BASE` 已占上限 96.5%，
      剩余余量 728 MiB，**信号被噪声淹没**
    - **必须改用链接器进程自身的 `VmHWM`**（只增不减，读到即峰值）
    - **测链接器前先确认目标真会链接**：曾挑 `full_system_test`，它首行是
      `#![cfg(feature = "broken_tests")]`，默认特性下整个文件被编译掉，
      touch 它不触发链接，测出 `RSS=0`。**零不是「链接器省」，是「压根没链接」**
- [x] **`test_persist_survives_buffer_recreation` 竞态，已修复** (2026-10-04): 确定性复现 5/30 轮丢失 → 修复后 0/30
    - 真因：`record_cognitive_trace` 是同步函数，只 `send` 就返回；无任何 await 持久化的手段
    - `ExecutionManager` 被 `Arc` 持有且克隆进 `SelfProgrammingManager`/`StrategicManager`，**`Drop` 不是可靠刷盘点**
    - 修复：channel 元素改为 `TracePersistCmd`（`Record` / `Flush`），`Flush` 用 oneshot 回执作 FIFO 屏障
    - 接入 `MySisyphusBridge::shutdown()`，让 SIGINT/SIGTERM 真实路径排空队列
    - clippy 报了 `large_enum_variant`，装箱 `Record` 载荷修掉（`ExecutionEvent` 比 oneshot sender 大得多）
    - **本条是生产缺陷**：bridge 关闭时丢跨会话认知追踪历史，不只是测试问题
- [x] **CI `toolchain-e2e` 会直接失败：已修** (2026-10-04): 移除 `cache: maven`
    - `setup-java@v4` 用 `**/pom.xml` 算缓存 key，而本仓库**没有签入任何 pom.xml**
      （Maven 探针是测试里 `TempDir` 现造的）
    - `cache.ts:96-100` 对「无文件匹配」是 `throw` 而非 warning → job 必红
    - Maven 本身**不需要安装**：runner 镜像（Ubuntu 24.04）预装 Maven 3.9.16
- [x] **`language_backend_guards.rs` 注释说「拉起 vitest」，实际无 vitest 用例** (2026-10-04)
    - 13 条全是 go / pytest / mvn；Node 侧 3 个缺陷靠 `node.rs` 解析器单测覆盖
    - 注释已改，避免后人误以为 Node 有真实工具链 e2e
- [ ] **本机 pytest 缺失会导致 e2e 假红** (2026-10-04 踩坑): 已用 `git stash` 对照确认非代码回归
    - 现象：6 条 pytest 用例报 `passed=0 failed=0`（后端跑 `python3 -m pytest` 直接报错）
    - 本机 `python3` 是 mise 3.14，**未装 pytest**；CI 里由 `pip install pytest` 保证
    - 本地复现真实 e2e 前必须先 `pip install pytest`，否则会把环境问题误当成代码回归
- [x] **「112s 慢测试」是我的误判** (2026-10-04): 测试实跑 2.80s，54s 全是编译
    - 我当时把 `cargo test` 的 `real 0m56s` 当成了测试耗时
- [x] **`sisyphus-bridge` 的 7 个 clippy 错误，已修** (2026-10-04): `cargo clippy -p sisyphus-bridge --all-targets -- -D warnings` 现为 **0 错误**
    - `field_reassign_with_default` 2 处 → 改用结构体字面量 + `..Default::default()`
    - `bool_assert_comparison` 2 处 → `assert_eq!(x, true)` 改 `assert!(x)`
    - `unused_mut` 1 处：`node_a` 只读无需 `mut`（旁边注释本就在解释 `node_b` 为什么要 mut）
    - 验证：clippy 0 错；`peer::experience_sync` 3 passed；`cluster_integration` 21 passed
    - 注：这些断言都在真实双节点集群测试里，不是空跑
- [x] **GitHub Actions CI 全绿** (2026-08-09, 27/27 jobs; 修复 linker/clippy/protoc/组件缺失)
- [x] **WebUI 中文化**
- [x] **eval 评测套件** (57 例, schema 校验 + mock/真实双模式)
- [x] **性能基准基线** (`docs/benchmarks/v2-baseline-2026-08-09.md`)

## 进行中 / 待办

### 阶段 4: 维护与质量 (2026-08)
- [x] **文档同步**: `book/` 与 `docs/` 反映最新的 crate 拆分架构与中文化
- [x] **依赖升级审计**: `cargo outdated` / `cargo deny check` 定期巡检
    - 门禁是 `cargo deny check`（读 `deny.toml`，CI `audit-sources` job 执行）。
      **不要用 `cargo audit` 判断本仓库是否通过**——它读自己的配置、不读
      `deny.toml`，对本仓库的 ignore 列表完全无感，会给出与 CI 不一致的结论。
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
- [x] **联邦学习**: 跨实例经验共享（ADR-016/017/018，peer/ExperienceSync/反熵/策略权重/可观测已落地；默认不新建独立聚合进程）。证据：`crates/sisyphus-bridge/tests/cluster_status_rest_test.rs::cluster_status_endpoint_reports_single_node_view` `make audit`
  证据：`crates/sisyphus-bridge/src/peer/experience_sync.rs::federated_aggregation_rejects_low_quality_experience`、
  `crates/sisyphus-bridge/src/peer/manager.rs::test_experience_sync_counters_separate_broadcast_and_inbound`、
  `crates/sisyphus-bridge/tests/cluster_status_rest_test.rs::cluster_status_endpoint_reports_single_node_view`（`make audit`）。
  - Phase A ✅（v0.4.0，ADR-016）：入站经验质量门槛
    （`evaluate_experience_quality` + `[cluster.federated]`，默认关闭）。
  - Phase B ✅（v0.4.0，ADR-017）：集群策略权重同步
    （`sisyphus_types::strategy_weights` + `StrategyWeightsHandler`，
    经 `StateProposal` 双向传播，`GET /api/strategy/weights` 暴露聚合视图）。
    证据：`sisyphus_types::strategy_weights::tests::merge_is_idempotent`、
    `sisyphus_bridge::peer::strategy_weights::tests::apply_remote_merges_remote_snapshot`、
    `cluster_integration.rs::test_two_node_strategy_weights_propagate_and_merge`。
  - Phase C ✅（v0.4.0，ADR-017 Amendment）：3 节点链式拓扑收敛
    （`apply_remote` 接受后转发，多跳传播）+ 变异验证 2 处。
    证据：`cluster_integration.rs::test_three_node_strategy_weights_converge_across_chain_topology`、
    `sisyphus_bridge::peer::strategy_weights::tests::apply_remote_forwards_only_accepted_snapshots`。
  - Phase D ✅（v0.4.0，ADR-017 Amendment 2）：节点槽位 TTL——收到 `PeerLeft`
    时丢弃该节点槽位（视图代表当前集群，非历史流水）。证据：
    `sisyphus_types::strategy_weights::tests::remove_node_drops_only_that_slot`、
    `sisyphus_bridge::peer::strategy_weights::tests::peer_left_drops_node_slot_and_keeps_others`。
  - Phase E ✅（v0.4.0，ADR-017 Amendment 3）：执行路径消费 `best_strategy`
    （`[cluster.strategy_selection]`，默认关闭，`min_uses = 5`；决策矩阵见
    `sisyphus_types::strategy_weights::select_strategy`）。
    证据：`strategy_weights::tests` 决策矩阵 10 个、`MetaLearningSynthesizer`
    执行路径单测 3 个（`crates/sisyphus-intel/src/synthesis.rs::tests::test_meta_synth_*`）。
  - Phase F ✅（v0.4.0）：转发放大量化第一步——发送计数 + 内容未变跳过广播。
  - Phase G ✅（v0.4.0，ADR-017 Amendment 4）：来源标注（proto `source_node_id`）+ 自回环早退
    + 来源/载荷一致性校验 + 空来源兼容；**按对端定向转发仍推迟**（见 ADR-017 Amendment 4
    评估结论：结构性改动，量化后不推荐在当前规模实施）。
  - Phase D 遗留（治本方案）✅：`StrategyWeightsHandler` 周期对账已实现（
    `list_peers()` 与 `seen_members` 比对，回收「曾见过成员、已离线」的槽位），
    单测 `peer::strategy_weights::tests::reconcile_offline_nodes_drops_orphaned_known_slots`
    覆盖；事件路径（`PeerLeft` → `forget_node`）由 Phase D 已覆盖。
  - 6.1 ✅（2026-11-08）：`GET /api/cluster/status` 只读协调状态（`operationId: getClusterStatus`），
    证据：`crates/sisyphus-bridge/tests/cluster_status_rest_test.rs::cluster_status_endpoint_reports_single_node_view`。
  - 6.2b/6.3 ✅（2026-11-08）：经验同步计数/拒绝分解 + 排障 runbook（只读），
    证据：`crates/sisyphus-bridge/src/peer/manager.rs::test_experience_sync_counters_separate_broadcast_and_inbound`、
    `crates/sisyphus-bridge/src/peer/experience_sync.rs::federated_aggregation_rejects_low_quality_experience`。
- [x] **多实例协调与经验聚合（v0.4.x 范围收口）**：按 ADR-018，复用 `StateProposal`/`ExperienceSync`/CRDT/反熵数据面 + `crates/sisyphus-bridge/tests/cluster_status_rest_test.rs::cluster_status_endpoint_reports_single_node_view` `make audit`
  本地查询/`/api/cluster/status` 观测视图；**默认不新建独立聚合进程**。强一致 `propose/quorum` 控制面
  未启用生产路径（`proposer: None`/语义未定义场景不得虚假成功）。证据：`docs/adr/018-multi-instance-coordination-and-experience-aggregation.md`（`make audit`）、
  `crates/sisyphus-bridge/tests/cluster_status_rest_test.rs::cluster_status_endpoint_reports_single_node_view`。
  - [ ] 6.5（条件触发）强一致控制面（默认租约方案进行中，门禁关闭；A=gRPC/内部+只读状态，B=不接入调度器）：ADR-019/门禁/`ProposeControl`/租约状态机+`ControlPlaneCoordinator`+`GrpcPeerProposer`+`ClusterRuntime`/`main`/`MySisyphusBridge` 装配已落地。快 CI：`control_plane.rs::{ControlPlaneState::apply,acquire_renew_release_and_fencing,acquire_rejected_when_held,request_id_idempotent_replay}` + `quorum_tests::{n3_quorum_acquire_succeeds_with_majority,n3_quorum_fails_when_majority_unreachable,n3_request_id_idempotent_across_coordinator,n3_renew_release_and_fencing_via_coordinator}`；`consensus.rs::{test_propose_rejected_as_leader_without_proposer,test_grpc_peer_proposer_propose_control_disabled_gate,test_grpc_peer_proposer_quorum_n3_mock}`；专用运行器 `#[ignore=requires dedicated runner]`：`control_plane.rs::quorum_tests::{n3_partition_heal_rejoin_address_update,rollback_gate_off_rejects_control_plane_writes,n3_minority_cannot_commit_when_quorum_unreachable}`。TLS/mTLS 与 `SubmitControlLease` 鉴权已接线（`peer::tls` + `ClusterServiceImpl` allowlist enforcement；`tests/control_plane_mtls_test.rs` 4 项 loopback 证据；rustls provider 显式安装；`make gen-control-plane-test-certs` 生成测试 PKI）。**仍待 ADR-019 准入**：协调节点 `(key,request_id)`/不确定提交指引、生产连接池/成员变更深度集成与受门禁调用入口、真实证书/SAN/secret 与共享端口 TLS 部署语义、专用运行器分区/leader 变更/混沌/重启/TLS 长稳与证书轮换证据与回滚演练、REST 写入（如需）；`cluster.control_plane.enabled=false` 默认，未满足前不得生产启用。

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
- [x] **基准基线刷新**: 轻量基准已刷新（`docs/benchmarks/lightweight-baseline-2026-11-08-post-v040.md`，post-v0.4.0；500 节点 DAG ~1.2–1.3ms，无 lazy-load 需求）。完整 `make perf-analyze`/critcmp 同机对比留待固定 runner（无测试：性能快照文档，非行为断言）。
- [x] **TUI/CLI 冷启动 profile**: release `sisyphus` 非交互启动 `--help/--version` ~0.00s wall、~8.4–8.9MB RSS，16M 二进制（`docs/benchmarks/cli-startup-profile-2026-11-08.md`）。交互式 TUI 首帧需 PTY harness，留待后续（无测试：性能快照文档，非行为断言）。
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
- [x] **CHANGELOG.md 启动**: 当前 320 个本地提交无 CHANGELOG，按 keep-a-changelog 格式补 0.x.x（无测试：文档条目，由 `make release-check` 覆盖一致性）
  - 已建立根目录 `CHANGELOG.md`（keep-a-changelog 1.1.0 + SemVer），含发布流程模板；发版前 `make release-check` 校验版本/CHANGELOG 一致性
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
- 7.3: ✅ 基准基线刷新（2026-11-08）/ DAG 渲染（基准已覆盖）/ ✅ TUI/CLI 冷启动 profile（2026-11-08）
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
- [x] **启动时全量 sync**: 新节点加入时请求所有 peer 的 experience —— 证据：`crates/sisyphus-bridge/tests/cluster_integration.rs::test_new_node_pulls_history_via_anti_entropy`（新节点经 ADR-010 版本向量反熵拉齐既有多条经验）
- [x] **运行时增量 sync**: 本地 store 变更后主动广播到 peer

#### 9.1.2 端到端集成测试
- [x] **2 节点 sync 测试**: `cluster_integration.rs` 扩展（2026-10-02）
  - [x] 生产拨号器真的把事件送到对端（`test_shipped_dialer_delivers_events_to_peer`，直接锁定三处丢弃缺陷）
  - [x] gRPC 往返隔离验证（`test_experience_sync_survives_grpc_round_trip`）
  - [x] 节点 A 写入 → 节点 B 可见，且反向亦然（`test_two_node_experience_sync_round_trip_persists`）
  - [x] 并发写入冲突：CRDT 版本高低裁决（`test_newer_remote_version_wins_concurrent_write`）
  - [x] 重复投递幂等（`test_duplicate_delivery_does_not_duplicate_experience`）
- [ ] **3 节点 Docker Compose**: 本地多节点验证
- [x] **断线补偿**: 重连后按 vector clock 差量补齐（事件即发即失，断线期间的经验会永久丢失）—— 证据：`crates/sisyphus-bridge/tests/anti_entropy_integration.rs::reconnect_delivers_only_the_gap`（只补断线期间的缺口，不重传已持有的行）
- [x] **chaos 扩展**: 网络分区 + 节点重启后 sync 恢复 —— 证据：`crates/sisyphus-bridge/tests/anti_entropy_partition_convergence.rs::partition_heal_converges_both_sides_via_bidirectional_sync`（分区痊愈后双侧并集收敛）、`crates/sisyphus-mem/tests/experience_id_collision.rs::sequence_resumes_from_store_after_restart`（序号从 store 恢复，重启后不会归零导致主键碰撞丢行）

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
- [x] **ADR 索引**: 文档化所有架构决策（无测试：文档条目，见 `docs/adr/README.md`）
  - 已建立 `docs/adr/README.md`：14 份 ADR 的状态/日期索引、编号冲突说明（`0005` vs `005`，不重命名）、反熵链路阅读顺序
  - 已接入 `book/src/architecture.md` 的 ADR 表（含 007–014），链接改为绝对 URL（mdbook 相对链接会 404）
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
- [x] **孤儿源文件扫描**: `scripts/audit_orphans.py` + `make audit-orphans` + CI。已端到端验证（移除 `mod tests;` 即被抓出）
- [x] **忽略测试理由审计**: `scripts/audit_ignored.py` + `make audit-ignored` + CI。`#[ignore]` 必须指向外部前置条件，不接受「测试当前失败」。合成用例验证：拦截 `requires dedicated runner` 与裸 `#[ignore]`，放行诚实理由与注释中引用的属性
- [x] **解除 2 处失效的 `#[ignore]`**: `bridge_integration_test` 中两处 `#[ignore = "requires dedicated runner"]` 的真实原因是测试失败（内核未注入、失败被吞成成功），本轮修复后已自包含通过；保持忽略会让本轮修复的失败传播语义在 CI 中零覆盖
- [x] **6 处从不编译的文档示例改为 `no_run`**: `peer/service.rs`、`config_store.rs`(2)、`node.rs`、`core/lib.rs`、`formal_verification.rs`。查出 3 处真实文档缺陷（`.await` 用在同步函数 `ConfigStore::load` 上、示例变量未定义、缺导入），并修掉 `ExecutorRouter::register` 重复的文档注释
- [x] **修复 `sisyphus-cli` `resolve_backend` 三份副本**: `build.rs`/`run.rs`/`diagnose.rs` 各一份 36 行副本且已漂移，收敛为 `build.rs` 的 `pub(crate)` 单一实现
- [ ] **重复实现扫描（尝试失败，未采用）**: 正则 + 字节级比对抓不住「复制后各自加注释」这一真实漂移形态；需 `syn` 级 AST 比对。详见状态报告 6.1
- [ ] **文档示例围栏审计（尝试失败，未采用）**: 「围栏前须有理由」无法区分理由与普通说明文字，合成用例即证伪；不可判定的门禁只会制造已覆盖的错觉。详见状态报告 6.2
- [x] **TODO 勾选门禁**（2026-11-04）: `scripts/audit_todo.py` + `make todo-audit` + CI `audit-sources`。勾选项必须附可解析证据（真实测试名 / make 目标 / 脚本）：引用的测试须真实存在、带测试属性、未被 `#[ignore]`；新增勾选无证据即拦截，历史条目登记在 `scripts/todo_baseline.txt`（ratchet，只减不增）。自检 14 个判别用例：`scripts/audit_todo.py --self-check`

### 10.2 阶段 9 剩余
> 协议已定：见 [ADR-010](docs/adr/010-experience-anti-entropy.md)（版本向量反熵，挂进现有双向 `Stream`，不新增 RPC）
> 前置缺陷 P1 已修复，行标识现在编码来源与计数，紧凑摘要前提成立

- [x] **ADR-010**: 跨节点经验的反熵补偿方案——dial 方发版本向量，peer 只回送差量，分页有界
- [x] **P1 经验行标识碰撞，同秒写入静默丢数据**: 主键改为 `exp:{ts}:{origin}:{counter}`，接收端采用发送方主键；并连带修掉「本地 CRDT 时钟退化」（version 恒为 1、vc 计数恒为 1）。新增 `experience_id_collision.rs` 两例回归。详见 ADR-010「前置缺陷 P1」
  - [x] **重启后序号恢复**: `VectorExperienceBank::new` 从 `store.max_local_counter(origin)` 恢复，写入计数不再归零；回归 `sisyphus-mem/tests/experience_id_collision.rs::sequence_resumes_from_store_after_restart`
  - [ ] **存量旧主键**: `exp:{timestamp}` 旧行不参与反熵摘要；0.x 采用清库而非迁移，需在发版说明中提示
- [x] **启动时全量 sync**: 由 ADR-010 版本向量反熵覆盖——流建立后 dial 方立即发 `SyncRequest`，peer 只回送差量；`cluster_integration.rs::test_new_node_pulls_history_via_anti_entropy`
- [x] **断线补偿**: 重连后按 vector clock 补差量；`anti_entropy_integration.rs::reconnect_delivers_only_the_gap` + `cluster_integration.rs::catchup_signal_sent_while_reconnecting_is_still_consumed`
- [x] **`ExperienceSyncHandler` 关闭语义加固**: 任务全程持有 `Arc<PeerManager>`，Sender 存活由类型系统保证；`Closed` 分支由 `warn!` 提升为 `error!`
- [x] **入站事件转发收敛**: `event_tx`（本地产生，拨号器订阅）与 `inbound_tx`（远端收到，仅 handler 订阅）分离，回弹在结构上不可达
- [ ] **3 节点 Docker Compose** / **网络分区恢复** 验证

### 10.3 工具链
- [x] **并行链接保护**: 实测 19.5 GiB 下 `-j 2` 仍会 OOM、`-j 1` 稳定；`test-all`/`test-tools`/`test-bridge*`/`test-cli-handlers` 已统一带 `CARGO_BUILD_JOBS=1`，Makefile 头部记录约束（直接敲 `cargo test` 仍需自行加 `-j 1`）
- [ ] **认知循环 v3 (ADR-010)**: 先写问题陈述再谈方向，当前 TODO 9.2 只有候选词，不足以支撑决策

### 10.4 多语言测试守卫（2026-10-04）
> 详见 [状态报告 2026-10-04](docs/project-status-2026-10-04.md)。提交 `6c3a52e`（+936/−66）。

- [x] **Go 守卫：多包部分构建失败被漏判**: `go test ./...` 在某包构建失败时仍执行其余包，stdout 同时有 `--- PASS` 与 `FAIL\t<pkg> [build failed]`，「没有 PASS 行」判据必然漏判。改用明确的 `[build failed]` 标记
- [x] **Python 解析器：全部通过却报 `passed=0`**: `strip_suffix(" passed")` 对 `1 passed in 0.36s` 永不命中（整行以 ` in 0.36s` 结尾）。改为逗号切分取数字+标签，`error` 归 failed、`skipped` 归 ignored
- [x] **vitest 解析器：混合结果报 `passed=2 failed=0`**: `Tests 1 failed | 1 passed (2)` 同一行兼含两词，旧代码先匹配 `passed`。重写为优先解析权威汇总行，且只认 `Tests` 行（`Test Files` 行格式相同但语义是文件级）
- [x] **vitest 套件加载失败守卫从不触发**: `Failed Suites` 在 stderr 且带 ANSI，原守卫只看 stdout。改为合并 stdout/stderr、解析前内部去色
- [x] **真实工具链 e2e 回归**: 新增 `tests/language_backend_guards.rs`，13 条真实拉起 `go` / `python3 -m pytest` / `mvn`，覆盖 Go 单包/多包/通过/失败、Python 全过/混合/跳过/两类收集错误/fixture 报错/失败、Maven 编译失败/全通过
- [x] **`TestResults` 增加执行状态维度**（治本）: `TestOutcome { Ran, NoTests, NotExecuted }` + `TestResults::outcome()`；CLI `test_status_label` 改为 `match outcome()`，不再靠 `failed == 0` 反推。commit `fd7cca7`，3 条 outcome 单测 + 9 条 CLI 判定单测
- [x] **e2e 纳入 CI**: `ci.yml` 新增 `toolchain-e2e` job，13 条 `#[ignore]` 用例以 `--ignored --test-threads=1` 在装有 go / python+pytest / JDK / Maven 的 runner 上执行
- [x] **清理环境告警**: `executor_failure_semantics.rs` 已由 `fetch_update` 改用 `try_update`（clippy 无弃用告警）；`.cargo/config.toml` 的 `test-all` 别名补 `-j 1`（嵌套 cargo 仍由 `make test-all` 的 `CARGO_BUILD_JOBS=1` 覆盖）

---

**更新日期**: 2026-11-04
**当前重点**: 10.1 勾选门禁已落地（`make todo-audit`）；剩余为 3 节点 Docker Compose / 网络分区验证、mutation/fuzz 扩展
**前置条件**:
- 阶段 9.1 经验同步生产链路已打通并有端到端验证 ✅
- 跨节点经验同步相关缺陷全部修复并回归通过 ✅
- 工作区全量回归 4750 通过 / 0 失败（`cargo test-all -j 2`，排除 bridge/sandbox）✅
- 多语言测试守卫 5 缺陷修复 + 13 条真实工具链 e2e 全绿，并接入 CI ✅
