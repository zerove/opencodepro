# TypeScript/JavaScript AI 开发规范 (TS/JS AI Spec)

## 1. AI 协同编码标准 (Coding Standards)
- **强类型强制 (Strict Typing)**：在所有 TS 项目中强制 AI 使用 `interface` 或 `type` 定义数据结构，禁止使用 `any`。
  - *理由*：TS 的类型系统是 AI 最好的“护栏”，能让 AI 在生成代码时自动发现逻辑矛盾。
- **函数式倾向 (Functional Preference)**：引导 AI 优先使用纯函数、不可变数据 (`readonly`) 和高阶函数 (`map`, `filter`, `reduce`)。
- **组件原子化**：在前端开发中，强制 AI 将组件拆分为 $\text{UI-only}$ 和 $\text{Logic-only}$ (Hooks) 两个层级。

## 2. 定向提示策略 (Prompting Patterns)
- **类型推导增强**：*“请分析当前上下文，为这个新函数定义最精准的 TypeScript 类型，并确保它与现有的接口完全兼容。”*
- **异步流优化**：*“请将此异步逻辑重构为更加鲁棒的模式（例如使用 `Promise.allSettled` 或 `async-generator`），并处理可能的超时情况。”*
- **Zod 校验集成**：*“请为该 API 响应编写 Zod Schema，并实现一个运行时验证层，确保 AI 生成的数据符合该 Schema。”*

## 3. AI 质量验收清单 (Validation Checklist)
- [ ] **类型覆盖度**：是否消除了所有不必要的 `any`？
- [ ] **状态管理分析**：AI 生成的状态变更逻辑是否会导致不必要的组件重绘？
- [ ] **副作用隔离**：所有的 Side Effects 是否都被正确地封装在 `useEffect` 或类似的生命周期钩子中？
- [ ] **异步安全**：所有的 `await` 调用是否都有相应的 `try-catch` 或 `.catch()` 处理？

## ❌ 常见 AI 误区 (Anti-Patterns)
- **过度使用 `any`**：AI 为了快速实现功能经常使用 `any`。$\rightarrow$ **正确做法**：使用 `unknown` 或定义精确的 `interface`。
- **闭包陷阱**：在循环中创建异步回调时忘记处理闭包变量。$\rightarrow$ **正确做法**：使用 `let` 或将逻辑封装在独立函数中。
- **状态碎片化**：将原本可以通过一个对象管理的多个 `useState` 分开定义。$\rightarrow$ **正确做法**：使用 `useReducer` 或组合状态对象。
