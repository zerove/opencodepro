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