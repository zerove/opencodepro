# Sisyphus 测试套件隔离与 OOM 缓解计划

**日期**: 2026-11-22  
**状态**: 规划阶段（Plan Phase）  
**目标**: 针对 `sisyphus-tests` 和 `evals` 等高资源消耗/重型测试套件，制定系统的隔离、条件编译（如 `#[ignore]` 策略）、内存剖析及并发度调优方案，彻底消除 CI 及本地开发中的 OOM 与资源争抢风险。

---

## 一、 背景与痛点分析

在 Sisyphus 认知引擎 Rust 工作区中，部分测试套件和评测组件（如 `sisyphus-tests`、`sisyphus-sandbox`、`evals` 中的高负载仿真与静态分析工具）具有以下资源特征：
1. **重型依赖引入**：`sisyphus-tests` 及沙箱相关 crate 涉及 `wasmtime`、`cranelift` 等庞大编译单元和运行时内存开销。
2. **状态共享与并发冲击**：集成测试（如 `single_node_full_stack.rs`、`cross_project_sharing.rs`）在并行执行时会产生大量并发任务、持久化文件 I/O 以及异步 runtime 线程池上下文切换，容易在资源受限环境（或标准 CI Runner）中触发 OOM（Out Of Memory）。
3. **CI 与默认构建门禁隔离**：当前 workspace 已将 `sisyphus-bridge`、`sisyphus-sandbox`、`sisyphus-sandboxd` 和 `sisyphus-tests` 从默认的 `cargo check-all` / `cargo test-all` 中排除（通过 `.cargo/config.toml` 和 `Makefile`），但开发者在全量验收或运行 `cargo test --workspace` 时仍可能遭遇内存峰值超出预期。

---

## 二、 核心优化策略

### 1. 测试套件级解耦与显式激活（Explicit Opt-in）
- **维护默认排除边界**：继续严格保持 `sisyphus-tests`、`sisyphus-sandbox` 等重型 crate 在日常快速检车 (`cargo check-all`) 中的排除状态。
- **分层执行标准**：
  - **L1 快速迭代**：`cargo test -p sisyphus-core --lib`（单元测试，秒级响应，低内存）。
  - **L2 业务集成**：`cargo test -p sisyphus-tests`（单独显式执行，需配合 `-j 1` 或控制并发度）。
  - **L3 评测与稳定性**：`cargo run -p evals` / `cargo test --manifest-path evals/Cargo.toml`。

### 2. 条件编译与大内存测试的 `#[ignore]` 策略
- **长耗时/高内存用例标记**：对于涉及完整认知循环、多节点沙箱或大规模嵌入模拟的集成测试，必须标注 `#[ignore = "requires dedicated runner or high memory"]`。
- **按需唤醒（Selective Unignore）**：在专有集成测试流水线中，通过 `cargo test -- --ignored` 显式激活。

### 3. Cargo 编译与测试并发度调优（Profile & Concurrency Control）
- **Codegen Units 极致调优**：已在 `Cargo.toml` 中针对 `wasmtime`、`cranelift`、`reqwest`、`hyper`、`h2`、`syn` 等重型依赖配置了细粒度 `codegen-units`（如 `cranelift-codegen` 设为 256，`wasmtime` 设为 64），以平衡链接期内存和编译速度。
- **并发度压制（Jobserver Limit）**：在执行重型测试时强制限制并发任务数（如 `cargo test -p sisyphus-tests -- --test-threads=1` 或 `MAKEFLAGS="-j2"`），避免 Rust 编译器和测试进程同时并发膨胀。

### 4. 评测套件（`evals`）的内存隔离
- `evals` 下的各项静态检查（如 `no_panic_check.rs`、`metrics_coverage.rs`）与离线评测（如 `golden_sop/eval.rs`）采用零外部重型依赖设计（仅依赖 `regex` 等轻量库），应当与重量级运行时沙箱测试在执行流上解耦，避免交叉编译瓶颈。

---

## 三、 实施路线图

1. **阶段 1：基线盘点与清单记录**（当前）
   - 盘点 `crates/sisyphus-tests/tests/` 下所有集成测试的内存与耗时热点。
2. **阶段 2：策略落地与标记加固**
   - 审查高内存占用测试，为非必须默认运行的用例补充 `#[ignore]` 描述。
   - 优化 Makefile 与 CI 脚本中的测试调用命令，统一推荐带 `-j 1` 或 `--test-threads=1` 的安全运行方式。
3. **阶段 3：监控与回归验证**
   - 建立文档指引，确保新贡献者在运行重型集成测试时有明确的资源开销预期与规避手段。
