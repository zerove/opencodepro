# OpenCodePro 单机客户端自编程实战 Cookbook

本 Cookbook 提供单机环境下的端到端自编程（Self-Programming）实战案例，演示 Sisyphus 如何在本地识别工具缺口、自主生成代码、编译、测试并注册为可用技能。

---

## 场景：自动生成一个本地日志分析与过滤工具

假设您在开发一个本地 Rust 或 Web 项目时，需要对特定格式的服务器日志进行高频过滤与统计，但当前工具箱中没有现成的解析工具。

### 步骤 1：触发任务与检测缺口
在 TUI 客户端 (`sisyphus-cli`) 中输入任务目标：
```
"Analyze server.log and extract all ERROR entries with frequency count"
```
认知引擎在执行 SOP 阶段发现缺少专用解析器工具，自动定义 `ToolGap`：
- **Tool ID**: `local/log_analyzer`
- **Required Capability**: `Parse log files and aggregate error frequency`
- **Success Criteria**: `Returns valid JSON summary of error counts`

### 步骤 2：自主生成与编译
`SelfProgrammingManager` 启动自我编程循环：
1. **代码生成**：调用配置的 LLM 生成 Rust 工具源码。
2. **WASM 编译**：通过 `WasmCompiler` 将源码安全编译为 `log_analyzer.wasm` 字节码模块。

### 步骤 3：沙箱验证与多轴质量评分
系统自动对新生成的工具运行：
- **单元与集成测试** (`run_regression_tests`)
- **多轴质量评估** (`evaluate_quality`)：
  - 测试覆盖率权重 (35%)
  - 规范完整度权重 (25%)
  - 沙箱稳定性权重 (25%)
  - 执行效率权重 (15%)
当**加权总分 $\ge 0.6$** 且**规范完整度 $\ge 0.8$** 时，才通过质量门禁。

> 规范完整度是硬前置条件：缺少 `success_criteria` 的工具无法被判定“做对了没有”，
> 即使测试恰好全绿也不会被自动注册为技能（`QualityReport::meets_registration_bar`）。

### 步骤 4：资产沉淀与技能注册
1. **沉淀**：工具二进制与规范被持久化到本地资产库 (`AssetManager`)。
2. **注册**：自动打包为 `SkillBundle` 并注入 `SkillStore`。
3. **即时调度**：系统在后续 SOP 步骤中直接调用刚刚“自己写出来”的 `log_analyzer` 工具完成任务。
