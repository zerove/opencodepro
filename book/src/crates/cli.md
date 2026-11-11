# sisyphus-cli — 命令行界面

CLI 二进制提供了从终端直接访问 Sisyphus 工具与代理的能力，支持一次性命令与交互式 TUI 模式。

## 安装

```bash
cargo build --release -p sisyphus-cli
./target/release/sisyphus --help
```

> 注意：crate 名是 `sisyphus-cli`，产物二进制名是 `sisyphus`。

## 命令结构

```
sisyphus [OPTIONS] <COMMAND>
```

### 全局选项

| 标志 | 环境变量 | 说明 |
|------|-----|-------------|
| `-i`, `--interactive` | | 启动交互式 TUI 模式 |
| `-b`, `--bridge` | `SISYPHUS_BRIDGE_ADDRESS` | Bridge gRPC 地址（默认：`http://[::1]:50051`） |
| `--data-dir` | `SISYPHUS_DATA_DIR` | 待办/会话存储的数据目录 |
| `--config` | `SISYPHUS_CONFIG` | TOML 配置文件路径 |

## 文件工具

```bash
# 读取文件
sisyphus read path/to/file.txt

# 写入内容到文件
sisyphus write path/to/output.txt "file content"

# 执行 shell 命令
sisyphus exec "cargo test" --dir /path/to/project

# 网络搜索
sisyphus search "Rust async/await" -n 10
```

### 编辑后自动验证（post-edit verify）

`write`、`edit`、`apply_patch` 落盘后可按配置执行一次短验证，避免“文件写坏了但命令仍报成功”。
该能力通过环境变量开启，不改变 `Config` schema：

| 变量 | 说明 |
|------|------|
| `SISYPHUS_POST_EDIT_VERIFY` | 验证命令模板。含 `{files}` 时替换为改动文件列表（shell 安全引用）；`auto` 表示按类型推断：`.rs` → 在最近的 Cargo 包根目录执行 `cargo check --quiet`；未设置或空串表示关闭 |
| `SISYPHUS_POST_EDIT_VERIFY_TIMEOUT_SECS` | 超时秒数，默认 `120`；非法值或 `0` 回落默认 |
| `SISYPHUS_POST_EDIT_VERIFY_STRICT` | `1`/`true`/`yes`/`on` 时验证失败返回非零退出（阻断“成功”）；默认仅告警 |

```bash
# 对改动的 Rust 文件执行 cargo check（按文件定位包根目录）
SISYPHUS_POST_EDIT_VERIFY=auto sisyphus edit src/lib.rs "old" "new"

# 自定义模板 + 严格模式：验证失败即命令失败
SISYPHUS_POST_EDIT_VERIFY='rustfmt --check {files}' \
SISYPHUS_POST_EDIT_VERIFY_STRICT=1 \
  sisyphus write src/main.rs "$(cat src/main.rs)"

# 限制超时
SISYPHUS_POST_EDIT_VERIFY='cargo clippy --quiet' \
SISYPHUS_POST_EDIT_VERIFY_TIMEOUT_SECS=60 \
  sisyphus apply-patch src/lib.rs -i fix.patch
```

输出为单行摘要（`✓/✗/-` + 耗时 + 命令）；失败时附带输出尾部 20 行。

## 验证与修复

### verify — 有界验证-修复循环

`diagnose` 只报告、`fix` 只修一轮；`verify` 把两者串成有界循环：**验证 → 未通过则修复 → 再验证**，
直到达到门禁或用尽轮数，并逐轮输出错误/警告/耗时与修复结果。

```bash
# 只验证（等价于一次 check + lint）
sisyphus verify

# 零警告门禁 + 最多 3 轮自动修复
sisyphus verify --max-warnings 0 --fix --attempts 3

# 修复时同时应用 lint 修复；输出机器可读 JSON
sisyphus verify --fix --lint-fixes --json

# 指定后端/包/工作目录
sisyphus verify --lang node -p web -C ./frontend
```

| 选项 | 说明 |
|---|---|
| `--lang` | 语言后端（rust/node/python/go/java/gradle/fe）；省略时按目录自动检测 |
| `-p`, `--package` | 目标包 |
| `--all` | 全部工作区成员 |
| `--no-lint` | 跳过 lint，仅 check |
| `--fix` | 未通过时执行一次修复并重试；未指定时只跑一轮验证 |
| `--lint-fixes` | 修复阶段同时应用 lint 修复（Rust 下为 `cargo fix --clippy`） |
| `--attempts N` | 最大验证轮数（默认 3，仅 `--fix` 时生效；上限保证终止） |
| `--max-warnings N` | 允许的最大警告数，`0` 表示零警告门禁；省略表示不限制 |
| `--json` | 输出结构化结果（轮次、耗时、修复记录、最终计数） |
| `-C` | 工作目录 |

门禁判定：`errors == 0` 且 `warnings <= max_warnings`（未设置时不限制）。未通过时命令以非零退出，
可直接用于 CI 或编排节点的判定条件。

## 代理子命令

### librarian — 外部调研代理

搜索网络、获取文档、查找代码示例。

```bash
# 调研主题（web、docs、code 或全部）
sisyphus librarian research "Rust async" --scope all -n 5

# 从 URL 获取并展示文档
sisyphus librarian fetch-docs https://doc.rust-lang.org/std/fmt/

# 查找库或 API 的代码示例
sisyphus librarian examples "serde_json::from_str" -l rust

# 从 JSON 调研报告综合发现
sisyphus librarian synthesise findings.json
```

`librarian research` 通过 `--scope` 支持以下范围：
- `web` — 搜索 web 获取一般信息
- `docs` — 获取文档页面
- `code` — 查找代码示例
- `all` — 以上全部（默认）

### metis — 预规划分析代理

在实现前分析请求中的歧义与隐含需求。

```bash
# 分析内联请求文本
sisyphus metis analyse "Add user authentication with OAuth2 support"

# 从文件分析请求
sisyphus metis analyse --file request.txt
```

分析输出包括：
- **摘要**（Summary）— 请求的高层评估
- **歧义**（Ambiguities）— 需要澄清的方面，含严重级别与建议解决方案
- **隐含需求**（Implicit Requirements）— 可推断但未明确陈述的需求

### momus — 计划/代码/架构审查代理

审查计划、源代码与架构的完整性与一致性。

```bash
# 审查计划文件
sisyphus momus review-plan plan.md

# 审查源代码文件
sisyphus momus review-code src/main.rs

# 内联审查架构文档
sisyphus momus review-arch "The system uses microservices with message queues"

# 审查目录下所有 Rust 文件
sisyphus momus review-dir src/
```

## 任务与会话管理

```bash
# 添加待办项
sisyphus todo add "Fix login bug" -p high

# 列出待办事项
sisyphus todo list --status pending

# 创建新会话
sisyphus session create "Refactoring session"

# 列出可用会话
sisyphus session list
```

## Bridge 命令

```bash
# 通过 bridge gRPC 执行标准操作程序
sisyphus sop run goal:deploy_to_staging

# 列出可用 SOP
sisyphus sop list

# 检查 bridge 健康状态
sisyphus health
```

## 工作区管理

工作区提供隔离的项目环境，带有独立状态（SOP、会话、待办）。

```bash
# 创建新工作区
sisyphus workspace create "My Project" --project-id proj_alpha --root-dir /path/to/project

# 列出所有工作区
sisyphus workspace list

# 切换到工作区
sisyphus workspace switch <workspace-id>

# 显示当前工作区
sisyphus workspace current

# 删除工作区（带确认提示）
sisyphus workspace delete <workspace-id>

# 删除工作区（跳过确认）
sisyphus workspace delete <workspace-id> -y
```

工作区内持久化的所有数据（SOPs、会话、待办、经验）通过 `project_id` 隔离。切换工作区会改变活动的项目上下文，使后续命令在该项目的存储命名空间内操作。

## 交互式 TUI

启动交互式终端界面：

```bash
sisyphus --interactive
```