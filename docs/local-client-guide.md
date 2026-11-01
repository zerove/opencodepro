# OpenCodePro 单机客户端开箱体验与自编程指南 (Standalone Client Guide)

本文档面向希望在**单机环境（Standalone / Single-Machine）**下运行、评估和使用 OpenCodePro (Sisyphus 认知引擎) 的开发者与用户。

在本模式下，系统完全基于本地存储（SQLite WAL + L1 LRU）、嵌入式 Wasmtime 沙箱以及本地/云端 LLM 供应商，无需部署任何复杂的分布式集群或 P2P 节点。

---

## 一、 快速起步与零配置初始化 (Zero-Config Onboarding)

### 1. 安装与编译
确保本地已安装稳定版 Rust (1.99+推荐) 及 Cargo。构建单机 release 版本的 CLI / TUI 客户端：

```bash
cargo build --release -p sisyphus-cli
```

编译产物位于 `target/release/sisyphus-cli`。可将其软链或加入系统 `PATH`。

### 2. 工作区初始化
在您的项目根目录下运行初始化命令：

```bash
sisyphus init
```

该命令将自动为您创建：
- `.sisyphus/` 目录：存放本地状态数据库、知识资产与缓存。
- `sisyphus.toml`：单机配置文件模板（包含 LLM 供应商设置、认知循环参数、沙箱配置）。

---

## 二、 启动单机交互式 TUI 客户端

通过终端一键启动基于 Ratatui 的精美 TUI 界面：

```bash
sisyphus-cli --interactive
```

### 核心功能区：
1. **会话管理 (Sessions)**：支持多会话隔离、上下文历史追踪与一键切换。
2. **目标与 SOP 驱动 (Goals & SOP DAG)**：输入宏观目标后，系统自动将其拆解为 DAG 执行图并实时展示预测 $\to$ 执行 $\to$ 反思 $\to$ 演进四阶段循环状态。
3. **思维块折叠 (Thought Blocks)**：大模型的思考过程（`<thought>`）支持一键折叠或展开，方便阅读核心结论。

---

## 三、 核心亮点：本地自我编程循环 (Standalone Self-Programming Loop)

OpenCodePro 最强大的特性之一是**自主扩展工具链 (Self-Programming)**。在单机模式下，该闭环完全在本地安全运行：

```
[ 目标执行 ] ──( 发现工具缺口 ToolGap )──> [ SelfProgrammingManager ]
                                                    │
         ┌──────────────────────────────────────────┘
         ▼
[ 1. LLM 代码生成 (ToolCoder) ]
         │
         ▼
[ 2. WASM 模块编译 (WasmCompiler) ]
         │
         ▼
[ 3. 沙箱验证与回归测试 (ToolVerifier & TestRunner) ]
         │
         ▼
[ 4. 质量评分评估 (QualityReport >= 0.6) ]
         │
         ▼
[ 5. 知识资产沉淀 & 技能动态注册 (AssetManager & SkillStore) ]
         │
         ▼
[ 立即投入后续 SOP 调度使用！]
```

### 如何体验自我编程：
1. 在配置中正确设置您的 LLM API 密钥与端点（或通过环境变量 `LLM_API_KEY`、`LLM_MODEL`）。
2. 当执行复杂任务且当前工具箱无法满足时，系统会自动捕捉 `ToolGap`。
3. `SelfProgrammingManager` 自动生成工具代码、编译为 `.wasm`、运行本地测试、进行多轴质量评分（测试通过率、规范完整度、稳定性、效率）。
4. 评分合格后，自动封装为 `SkillBundle` 注册到本地 `SkillStore`，实现“边用边学、自主扩容”。

---

## 四、 本地宏观战略规划 (Roadmap Manager)

单机客户端内置了对复杂项目的多级战略规划支持：
- **Project (项目)**：宏观根实体。
- **Milestone (里程碑)**：关键交付阶段与验收标准。
- **Phase (阶段)**：有序或部分有序的目标集合。
- **Goal (目标)**：由认知引擎调度的原子执行单元。

通过 `sisyphus-roadmap` 模块，单机客户端不仅能执行当下指令，还能跨 Session 追踪长期项目进展，并在遇到阻碍时自动触发**战略反思与重规划 (Strategic Reflection & Re-planning)**。

---

## 五、 最佳实践与故障排查

1. **LLM 供应商回退链**：
   系统内置了指数退避、抖动及错误分级（Transient / Permanent / Auth）的重试机制，即使在网络抖动时也能保持稳定。
2. **嵌入不可用时的词法回退**：
   当向量嵌入服务不可用时，系统会自动平滑降级为基于 Jaccard 相似度的词法检索，确保历史经验查找永不中断。
3. **生产质量纪律**：
   单机核心运行时严格禁止 `unwrap()` / `expect()`，所有错误均通过强类型 `CoreError` / `TypesError` 规范传播，确保本地长时间运行零崩溃。
