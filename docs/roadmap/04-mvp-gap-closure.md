# MVP 缺口闭合计划 — OpencodePro 可用性冲刺

## 目标
将 opencodepro 从"功能完整"推进到"日常可用"，按对开发流程和产出的影响度优先级补齐缺口。

## 优先排序原则

| 权重 | 维度 | 说明 |
|------|------|------|
| P0 | **日阻塞** | 无此功能无法正常完成开发工作流 |
| P1 | **高频增益** | 每天使用多次，显著提升效率 |
| P2 | **集成增强** | 扩展使用场景但非必需 |
| P3 | **生态扩展** | 长期增值，不影响当前使用 |

---

## Phase 0 — 基础知识资产化（P0: 日阻塞）

### 0.1 智慧积累 — `.omo/notepads/` 机制

**为什么是 P0**: 如果没有跨 Session 知识保留，每次新 Session 都从零开始。这是 OMO 最核心的效能杠杆。

- `feedback.rs` 已有 `FeedbackStore` 模式（文件存储 + 按 Agent 分类），可作为 notepad 存储的参考
- `skill.rs` 的 `SkillLoader` 提供了目录扫描 + 文件解析模式，可作为 notepad 加载的参考
- 需要从零构建 `wisdom.rs` 模块：notepad 文件 CRUD + 注入到 Agent 上下文

**交付物**:
- `.omo/notepads/` 目录结构（conventions/successes/failures/patterns/experience）
- `NotepadManager` struct: `record()` / `list()` / `get()` / `delete()` / `inject()`
- `inject()` 返回格式化 context 块，用于注入 system prompt
- CLI: `wisdom record` / `wisdom list` / `wisdom show` / `wisdom delete` / `wisdom inject`

**QA 验证**:
- 工具: `cargo test --workspace` + CLI 手动测试
- 单元测试: ① `NotepadManager::new()` 使用临时目录
  ② `record("conventions", "test content")` → 文件被创建
  ③ `list()` → 返回包含 "conventions"
  ④ `inject()` → 返回字符串包含 `<wisdom>` 标签
  ⑤ `get("conventions")` → 返回完整内容含时间戳
- CLI 测试: ① `sisyphus wisdom record conventions "msg"` → 写入成功
  ② `sisyphus wisdom list` → 显示 conventions
  ③ `sisyphus wisdom show conventions` → 显示内容
  ④ `sisyphus wisdom inject` → 输出含 `<wisdom>` 块
- 预期: 所有操作在单元测试 + CLI 中覆盖

### 0.2 Session 自动上下文压缩

**为什么是 P0**: 长 Session（50+ 消息）会因 token 超限导致 LLM 退化、幻觉或完全中断。无压缩机制则无法完成大任务。

- `Message` 新增 `estimated_tokens` 和 `is_compressed` 字段
- 添加 `estimate_tokens()` 函数（字符数/4 简单估算）
- 添加 `session_compress()` 函数：超限时将历史消息合并为压缩摘要
- 压缩后消息标记 `is_compressed = true`，原始内容存入 metadata

**交付物**:
- `Message` 新增字段: `estimated_tokens: u64`, `is_compressed: bool`
- `CompressStrategy` enum: `Threshold(u64)` 默认 32000
- `session_estimate_token_count()` 函数
- `session_compress()` 函数（压缩 + 恢复）
- 压缩可见性："已压缩 N 条消息" 状态

**QA 验证**:
- 工具: `cargo test --workspace`
- 步骤: ① 创建 session + 添加 5 条消息
  ② `session_estimate_token_count()` → 返回 > 0
  ③ `session_compress()` 用低阈值 → 消息被压缩
  ④ 压缩后 `message_count` 不变，`is_compressed = true`
  ⑤ 原始内容可从 metadata 恢复
- 预期: 压缩不丢数据，可逆，向后兼容

**预估**: 2 周

### 0.3 Session 连续性（checkpoint 机制）

**为什么是 P0**: Session 中断（网络崩溃、LLM 超时、机器重启）后无法精确恢复当前状态。当前 session.rs 只能恢复到消息级，丢失执行中的上下文。

- `SessionCheckpoint` 结构体：session_id, last_message_idx, timestamp, metadata
- `session_save_checkpoint()` + `session_load_checkpoint()` + `session_clear_checkpoint()`
- checkpoints 存储在 `.sisyphus/checkpoints/{session_id}.json`
- `session_add_message()` 自动清理 checkpoint（标记恢复完成）
- `session_continue()` 检测残留 checkpoint 并告警

**交付物**:
- `SessionCheckpoint` 结构体 + 序列化
- 3 个 checkpoint 管理函数
- checkpoint 自动清理逻辑
- `session_export()` / `session_import()` 函数（便携式 Session JSON）

**QA 验证**:
- 工具: `cargo test --workspace`
- 步骤: ① 保存 checkpoint → 文件存在
  ② 加载 checkpoint → 数据完整
  ③ 添加消息 → checkpoint 被清理
  ④ export → 生成独立 JSON 文件
  ⑤ import → 恢复完整 Session
- 预期: checkpoint 生命周期完整，export/import roundtrip 不丢数据

**预估**: 1 周

---

## Phase 1 — 工作流增强（P1: 高频增益）

### 1.1 Session 导出/导入

**为什么是 P1**: 跨机器工作是常态。当前 session 存储在本地 JSON，无法迁移。

- `opencode export <session_id>` → 导出为独立文件（含 messages + metadata）
- `opencode import <path>` → 导入到本地 session store
- 格式兼容：JSON 序列化，保留完整消息历史

**交付物**:
- CLI: `session export` / `session import`
- 单元测试：roundtrip 完整性

**QA 验证**:
- 工具: `cargo test --workspace` + CLI 手动测试
- 步骤: ① 创建 session + 添加 3 条消息
  ② `session export <id> /tmp/test-session.json` → 文件存在
  ③ `session import /tmp/test-session.json` → 新 session 创建成功
  ④ 新 session 消息数量 = 3，内容匹配
- 预期: export/import 完整 roundtrip

**预估**: 1 周

### 1.2 PR 工作流

**为什么是 P1**: Code Review 是开发核心环节。当前无 `opencode pr <number>` 等效命令。

- `sisyphus pr checkout <number>` — 检出 PR 到本地
- `sisyphus pr review <number>` — 启动 Momus/Oracle review 管线
- `sisyphus pr list` — 列出开放 PR
- 基于 `gh` CLI 封装（非从头实现 GitHub API）

**交付物**:
- CLI: `pr checkout` / `pr review` / `pr list`
- PR review 管线自动调用 Momus + Oracle

**预估**: 2 周

### 1.3 Hashline 编辑防冲突

**为什么是 P1**: 当前 `edit` 工具使用 exact string match，多人/多 Agent 协作时可能匹配错误。

- 为文件行引入内容哈希（LINE#ID 模式）
- 编辑时验证目标行哈希，冲突则拒绝编辑
- 渐进式：不影响现有 edit 逻辑，只在标注了 LINE#ID 的行启用

**交付物**:
- LINE#ID 标注生成
- 防冲突编辑模式
- 冲突时给出清晰提示（当前内容 vs 预期）

**预估**: 1 周

---

## Phase 2 — 平台集成（P2: 集成增强）

### 2.1 HTTP API / REST

**为什么是 P2**: 当前仅 gRPC，IDE 插件、CI/CD、Webhook 集成需要 HTTP。

- 在 sisyphus-bridge 上叠加 REST 层
- 可选方案：axum 添加 HTTP listener（保留现有 gRPC）
- Endpoint: `POST /api/execute`, `GET /api/sessions`, `POST /api/chat`

**交付物**:
- sisyphus-bridge HTTP listener（axum）
- OpenAPI schema（utoipa）
- CI 集成示例（GitHub Action workflow）

**预估**: 3 周

### 2.2 自定义工具（TS/Zod）

**为什么是 P2**: MCP + WASM 已是等效替代路径，但 TS/Zod 方式更符合前端生态习惯。

- 方案 A（推荐）：优化 MCP 工具的用户体验，降低注册门槛
- 方案 B：实现 `.opencode/tools/*.ts` 解析器，动态编译为 WASM 或 MCP 服务

**交付物**:
- MCP 工具快速注册 CLI: `tool register --mcp <command>`
- 可选：TS→WASM 编译管线

**预估**: 3 周

### 2.3 更多 LLM 提供商

**为什么是 P2**: 当前主要支持 OpenAI/Anthropic/本地模型，扩展至 75+ 提供商会显著增加选择灵活性。

- 集成 Models.dev / AI SDK 兼容层
- 提供商注册机制：`provider add <name> <api_url> <api_key>`
- 自动 fallback：主模型失效 → 备用模型

**交付物**:
- Provider 注册表 + CLI 管理
- 5-10 个主流提供商适配（Google/Groq/Cohere/Mistral/Together）
- 自动 fallback / 负载均衡

**预估**: 3 周

### 2.4 GitHub Action 集成

**为什么是 P2**: CI/CD 自动化是团队协作的基础设施。

- `sisyphus github action` — 生成 GitHub Action workflow 模板
- CI 模式：非交互式执行，结果直接输出到 Action log
- PR 自动 review action

**交付物**:
- GitHub Action workflow 模板
- CI 模式执行器
- PR review 自动触发

**预估**: 1 周

---

## Phase 3 — 生态扩展（P3: 长期增值）

### 3.1 TUI 体验增强

- 模糊搜索（Session/文件/命令）
- `@file` 引用自动补全
- `/slash` 命令面板
- 主题 / 配色定制

**预估**: 2 周

### 3.2 Web 界面

- 基于 axum + HTMX 的轻量 Web UI
- Session 列表 / 对话 / 文件浏览
- 路线图看板可视化

**预估**: 6 周

### 3.3 Session 分享

- 导出为公开链接（可选本地服务器）
- 只读分享 / 可评论分享

**预估**: 2-3 周

### 3.4 OAuth MCP

- MCP 服务器 OAuth 认证流程
- Token 管理 / 刷新

**预估**: 1-2 周

### 3.5 桌面应用

- Tauri 封装（macOS/Windows/Linux）
- 原生菜单 / 系统托盘 / 自动更新

**预估**: 8 周

---

## 路线图总览

```
Phase 0 — 知识资产化          (P0, 5 周)
  ├── 0.1 智慧积累 notepads      2 周  ← 启动
  ├── 0.2 Session 自动压缩        2 周  ← 并行
  └── 0.3 Session 连续性         1 周  ← 并行

Phase 1 — 工作流增强           (P1, 4 周)
  ├── 1.1 Session 导出/导入      1 周  ← 并行
  ├── 1.2 PR 工作流              2 周  ← 可并行
  └── 1.3 Hashline 防冲突        1 周  ← 并行

Phase 2 — 平台集成             (P2, 8 周)
  ├── 2.1 HTTP API               3 周  ← 可并行
  ├── 2.2 自定义工具             3 周  ← 可并行
  ├── 2.3 更多 LLM 提供商        3 周  ← 可并行
  └── 2.4 GitHub Action          1 周  ← 并行

Phase 3 — 生态扩展             (P3, 持续)
  ├── TUI 增强 / Web UI / 分享
  ├── OAuth / 桌面应用
  └── 持续迭代
```

**排除项**（暂不计划）:
- 桌面应用（Tauri）— P3 且非主线，投入产出比低
- Session 分享（公开链接）— P3，使用场景窄
- 配置变量替换 `{env:VAR}` — 可快速补但非急需

---

## 启动建议

1. **Phase 0 并行启动** — 0.1 智慧积累、0.2 压缩、0.3 连续性三者无依赖关系
2. **每 Phase 结束时验证可用性** — `cargo check --workspace && cargo test --workspace`
3. **Phase 0 完成后即可日常使用** — 知识保留 + 长 Session 管理 + 断点续传
4. **Phase 1 按需选做** — Session 导出/导入跨机器必做，PR 工作流按需
