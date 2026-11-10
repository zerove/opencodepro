# Sisyphus (opencodepro) vs OpenCode vs JCode 对标分析（单机场景，2026-11）

> 日期：2026-11-10  
> 范围：聚焦**单机全功能**与产品化切面（引擎优先、选择性产品化）；不将 ADR-019 专用 runner 真实网络证据与控制面生产启用纳入本轮对标结论。  
> 依据：仓库当前实现与文档（S1–S4 交付）、OpenCode/JCode 官方仓库 README（抓取时间 2026-11-10）。

## 1. 产品定位与核心差异

| 维度 | OpenCode (`anomalyco/opencode`) | JCode (`1jehuang/jcode`) | Sisyphus（本项目） |
|---|---|---|---|
| 定位 | 开源 coding agent 产品（CLI/TUI/桌面/生态） | 高性能多智能体 coding agent harness（低内存/快启动/Swarm/自举开发） | **认知引擎/执行平台**：DAG 认知循环（predict→execute→reflect→evolve）+ SOP 编排 + bridge/SDK/WebUI |
| 形态 | TS/Bun 多端产品，生态与文档成熟 | Rust 单体/多 crate，高并发会话与协作 | Rust workspace（多 crate），core/bridge/cli/webui/tools/mem/knowledge 等分层 |
| 单机强项 | 完整开发者交互体验（agent、工具、会话、MCP、桌面等） | 性能与资源效率（首响/增量会话内存）、Swarm 协作、自举开发 | 认知循环与可编排执行（SOP/DAG）、统一服务面（gRPC+REST）、可观测与 SDK/WebUI 契约 |
| 典型用户价值 | “即用”的编码代理 | “多 agent 高密度并行”的开发 harness | “可编排、可观测、可集成”的认知执行底座 |

## 2. 单机场景能力对照（摘要）

> 说明：✅ 已具备/可用；⚠️ 部分具备或依赖配置；❌ 明显缺失或非当前形态；“—” 不适用。

| 能力域 | OpenCode | JCode | Sisyphus（当前） | 证据/备注 |
|---|---|---|---|---|
| **基础交互入口** | CLI/TUI/桌面/Web（产品化） | TUI/CLI（高性能） | CLI/TUI + WebUI（axum+HTMX）+ REST/gRPC bridge | `crates/sisyphus-cli`, `webui/`, `crates/sisyphus-bridge/src/http` |
| **核心工具（文件/命令/搜索等）** | 成熟工具集与权限/会话体验 | 面向 agent 工作流的工具与协作 | 工具链丰富（tools-*）、CLI 本地工具实测可用；MCP 支持 | SN-01 验证；`tools-*` crates |
| **会话与任务流** | 会话管理成熟（产品体验） | 多会话/Swarm 协作 | 会话 API/CLI/WebUI 路径存在；契约已对齐 | bridge routes + WebUI/SDK |
| **执行模型** | Agent 驱动编码任务 | 多 agent 并行协作/自组织 | **SOP/DAG + 认知循环**（差异化核心） | `sisyphus-core`, execute/goal 路径 |
| **可观测性** | 产品内状态/日志体验 | 性能/运行状态导向 | 健康/指标/认知追踪与地图、集群只读状态、WebUI 面板 | `/api/health*`, `/api/metrics`, cognitive/cluster endpoints |
| **扩展与集成** | MCP/插件生态、SDK/HTTP 能力 | Provider/OAuth、自举扩展 | MCP、三端 SDK（TS/Py/Go）、REST/OpenAPI、WebUI | SDK 测试通过；OpenAPI/契约对齐批次 |
| **性能与资源（单机）** | 依产品形态而异 | **强调首响与增量会话低内存**（公开基准） | Rust 实现、可测基准体系存在；未以同口径对标 JCode 公开数据 | 需统一口径后再比较 |
| **多 agent 协作** | 代理/子任务能力（产品内） | **Swarm 原生协作**（同仓库冲突感知、消息、自治编排） | 编排能力以 SOP/DAG/工具与 agent 机制为基础；产品化协作体验待增强 | 与 JCode 形态不同 |
| **安全/隔离** | 权限与沙箱策略（产品侧） | 视配置而定 | sandbox/wasmtime/sandboxd 路径存在；单机受环境限制需分层验收 | S3 分层说明 |

## 3. 差距与机会（单机优先，面向 P1/P2）

### 3.1 相对 OpenCode 的主要差距（产品体验侧）
- **交互与工作流打磨**：会话/任务流的端到端顺畅度、错误归因与引导、TUI/CLI 体验细节与 OpenCode 成熟产品仍有差距。
- **生态与易用性**：提供商/模型接入体验、配置与上手路径的“开箱即用”程度有提升空间。
- **文档与示例的场景化**：从“能力存在”到“按场景可复制”的示例与教程仍需持续补齐（S3 已起步）。

### 3.2 相对 JCode 的主要差距（协作与性能体验侧）
- **多 agent 协作产品形态**：JCode 的 Swarm（冲突感知、消息、自治分工）是其核心体验；Sisyphus 需以 SOP/DAG/编排优势提供可替代的协作路径，而非直接复刻。
- **性能口径与感知**：JCode 以明确指标宣传（首响/增量内存）；Sisyphus 需建立单机可复现的性能/资源基线与用户可感知的优化点。

### 3.3 Sisyphus 的差异化优势（应强化）
- **认知循环 + SOP/DAG 编排**：适合复杂、多步骤、可审计的任务执行与复用。
- **统一服务与契约面**：bridge（gRPC/REST/OpenAPI）+ WebUI + 三端 SDK，利于集成与二次开发。
- **可观测性与治理基础**：认知追踪/地图、只读集群观测、指标与门禁体系，有利于平台化演进。

## 4. 结论（对标）

- **单机能力底座**：Sisyphus 已具备引擎、接入面、SDK/WebUI 与只读观测的核心闭环（S1–S4 证据），可作为“认知执行平台”在单机场景持续演进。
- **对标策略**：不追求与 OpenCode/JCode 的全量形态对齐；**以引擎与编排为主线**，选择性吸收 OpenCode 的产品化体验与 JCode 的协作/性能思路，转化为 P1/P2 的可验证增量。

## 5. 关联文档

- `docs/plans/single-node-full-functionality-audit.md`（S1）
- `docs/runbooks/single-node-operation-and-verification.md`（S2）
- `docs/runbooks/single-node-tools-and-dependencies.md` / `docs/examples/single-node-sdk-examples.md`（S3）
- `docs/plans/single-node-acceptance-matrix.md`（S4）
- `docs/plans/p1-p2-roadmap.md`（路线细化）
