# Jcode 开发规范 (Jcode Development Standard)

本规范定义了 Jcode 在本项目中执行所有开发任务必须遵循的标准流程和质量基准。本规范的目的是消除 AI 协作中的不确定性，确保项目在快速迭代的同时，保持极高的架构质量和上下文一致性。

## 核心哲学
**“文档即代码，状态即生命。”**
代码是结果，而引导代码生成的上下文（CONTEXT）和决策记录（ADR）是项目的核心资产。所有开发活动必须服务于资产的沉淀。

---

## 🔄 强制性工作流 (The Mandated Workflow)

对于任何功能开发、Bug 修复或架构调整，Jcode 必须执行以下闭环路径：

### 1. 上下文同步 (Context Sync) $\rightarrow$ **【必须执行】**
- **动作**：在开始任何任务前，必须读取 `CONTEXT.md` 和相关的 `ADR`。
- **目的**：确保当前行为与项目整体进度和既定架构决策一致，防止出现回归错误。

### 2. 设计先导 (Design-First) $\rightarrow$ **【复杂任务必须执行】**
- **动作**：在编写核心代码前，必须提交一份简短的 $\text{PRD}$ 或 $\text{Technical Design}$ 文档。
- **标准**：设计文档必须包含：核心逻辑描述、接口契约 (API Contract)、受影响的模块。
- **确认**：在用户确认或在 `ADR` 中记录后方可进入编码阶段。

### 3. TDD 迭代编码 (TDD & Iterative Coding) $\rightarrow$ **【必须执行】**
- **动作**：遵循 $\text{定义测试} \rightarrow \text{实现代码} \rightarrow \text{验证通过}$ 的循环。
- **原则**：禁止一次性提交巨大的代码块。必须采用“小步快跑”策略，每个 commit 仅解决一个原子功能点。

### 4. 自动化质量审计 (Automated Quality Audit) $\rightarrow$ **【必须执行】**
- **动作**：运行 `ai-context-sync audit` 和 `ai-context-sync guard`。
- **目标**：分别针对【语言开发规范】和【架构决策 (ADR)】进行自动化扫描。
- **处理**：若审计报告出现 `VIOLATION` 或 `CRITICAL` 项，必须在提交前完成修正。

### 5. 自我修正循环 (Self-Correction Loop) $\rightarrow$ **【必须执行】**
- **动作**：在提交最终结果前，必须运行所有相关测试。
- **闭环**：若测试失败 $\rightarrow$ 捕获错误 $\rightarrow$ LLM 修复 $\rightarrow$ 重新验证。
- **交付标准**：提交的代码必须在当前环境下通过所有单元测试。

### 6. 资产更新 (Asset Update) $\rightarrow$ **【必须执行】**
- **动作**：任务完成后，必须同步更新 `CONTEXT.md` 中的【当前进度】和【已完成模块】。
- **记录**：若涉及架构变更，必须同步创建一个新的 `ADR` 文档。

---

## 🛠️ 工具链标准 (Tooling Standards)

- **IDE**: 默认使用 **Zed**，遵循 `zed-config/` 中的配置。
- **上下文维护**: 使用 `AI-Context-Sync` 工具确保文档实时同步，并利用其 `audit` 和 `guard` 命令执行代码规范审计。
- **模板应用**: 所有的项目初始化必须基于 `templates/PROJECT_SKELETON.md`。
- **Prompt 策略**: 采用 $\text{Persona} \rightarrow \text{Context} \rightarrow \text{Task} \rightarrow \text{Constraint}$ 的结构化提示词。

---

## ✅ 交付物验收标准 (Acceptance Criteria)

一个任务被视为“已完成”必须满足以下条件：
1. [ ] **代码通过**：所有相关测试用例执行通过。
2. [ ] **文档同步**：`CONTEXT.md` 已更新至最新状态。
3. [ ] **决策记录**：关键变更已记录在 `ADR` 中。
4. [ ] **无冗余**：代码经过精简，移除了所有调试日志和临时注释。

---

## ⚠️ 违规处理
若 Jcode 在执行过程中跳过上述任何步骤，用户有权通过 `⚠️ 规范违规` 指令要求其回溯并补齐流程。
