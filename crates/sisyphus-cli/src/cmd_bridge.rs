use std::path::PathBuf;

use clap::Subcommand;

use crate::PriorityArg;

#[derive(Subcommand)]
/// Todo Commands.
pub enum TodoCommands {
    /// 添加新的待办事项
    Add {
        content: String,
        #[arg(short = 'p', default_value = "medium")]
        priority: PriorityArg,
    },
    /// 列出待办事项，可按状态过滤
    List {
        #[arg(short = 's')]
        status: Option<String>,
    },
    /// 更新待办事项
    Update {
        id: String,
        #[arg(short = 'c')]
        content: Option<String>,
        #[arg(short = 's')]
        status: Option<String>,
        #[arg(short = 'p')]
        priority: Option<PriorityArg>,
    },
    /// 按 ID 删除待办事项
    Delete {
        id: String,
        ///跳过确认提示
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
}

#[derive(Subcommand)]
/// Session Commands.
pub enum SessionCommands {
    /// 创建新会话
    Create { name: String },
    /// 列出所有会话
    List,
    /// 按 ID 获取会话
    Get { id: String },
    /// 按 ID 删除会话
    Delete {
        id: String,
        ///跳过确认提示
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// 向会话添加消息
    AddMessage {
        id: String,
        role: String,
        content: String,
    },
    /// 重命名会话
    Rename { id: String, name: String },
    /// 分叉会话 — 创建包含全部消息的新副本
    Fork { id: String, name: String },
    /// 继续会话 — 更新 updated_at 以调整排序
    Continue { id: String },
    /// 撤销会话中的最后一条消息
    Undo { id: String },
    /// 将会话导出到文件（json 或 html 格式）
    Export {
        id: String,
        output: PathBuf,
        ///输出格式：json（默认）或 html
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// 从 JSON 文件导入会话
    Import { input: PathBuf },
    /// 基于 token 阈值压缩会话
    Compress {
        id: String,
        ///可选 token 阈值（默认：32000）
        threshold: Option<u64>,
    },
}

#[derive(Subcommand)]
/// Plan Commands.
pub enum PlanCommands {
    /// 从需求文本创建计划
    Create {
        name: String,
        #[arg(short = 'd', default_value = "")]
        description: String,
        ///需求文件（以 "- " 开头的行将成为步骤）
        #[arg(short = 'f')]
        file: Option<PathBuf>,
    },
    /// 将计划保存到 JSON 文件
    Save { plan_file: PathBuf, output: PathBuf },
    /// 加载并显示计划
    Show { plan_file: PathBuf },
    /// 审查计划中的问题
    Review { plan_file: PathBuf },
    /// 批准计划
    Approve { plan_file: PathBuf },
}

#[derive(Subcommand)]
pub enum LspCommands {
    /// 获取文件的诊断信息
    Diagnostics {
        /// 要分析的文件路径
        file: PathBuf,
        ///语言 ID（例如 "rust"、"typescript"、"python"）
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        ///语言服务器命令（例如 "rust-analyzer"）
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        ///额外的服务器参数
        #[arg(last = true)]
        server_args: Vec<String>,
        ///工作区根 URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
    /// 跳转到符号的定义
    GotoDef {
        ///文件路径
        file: PathBuf,
        ///行号（从 0 开始）
        line: u32,
        ///列号（从 0 开始）
        col: u32,
        ///语言 ID
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        ///语言服务器命令
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        ///额外的服务器参数
        #[arg(last = true)]
        server_args: Vec<String>,
        ///工作区根 URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
    /// 查找符号的引用
    FindRefs {
        ///文件路径
        file: PathBuf,
        ///行号（从 0 开始）
        line: u32,
        ///列号（从 0 开始）
        col: u32,
        ///语言 ID
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        ///语言服务器命令
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        ///额外的服务器参数
        #[arg(last = true)]
        server_args: Vec<String>,
        ///工作区根 URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
    /// 获取某位置的悬停信息
    Hover {
        ///文件路径
        file: PathBuf,
        ///行号（从 0 开始）
        line: u32,
        ///列号（从 0 开始）
        col: u32,
        ///语言 ID
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        ///语言服务器命令
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        ///额外的服务器参数
        #[arg(last = true)]
        server_args: Vec<String>,
        ///工作区根 URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
    /// 列出文档符号
    Symbols {
        ///文件路径
        file: PathBuf,
        ///语言 ID
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        ///语言服务器命令
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        ///额外的服务器参数
        #[arg(last = true)]
        server_args: Vec<String>,
        ///工作区根 URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
}

#[derive(Subcommand)]
/// Mcp Commands.
pub enum McpCommands {
    /// 连接 MCP 服务器并显示信息 + 可用工具
    Info {
        ///服务器命令/URL，后跟可选参数。
        ///URL（http://...）→ HTTP 传输。
        ///否则视为 `<command>` `[args...]` → stdio 传输。
        #[arg(required = true)]
        connect: Vec<String>,
    },
    /// 调用 MCP 服务器上的工具
    Call {
        ///要调用的工具名
        tool: String,
        ///JSON 参数（可选，例如 '{"text":"hello"}'）
        args: Option<String>,
        ///服务器连接：`<url>` 或 `<command>` `[args...]`
        #[arg(required = true)]
        connect: Vec<String>,
    },
    /// 启动通过 stdio 暴露 Sisyphus 代理工具的 MCP 服务器
    Serve,
}

#[derive(Subcommand)]
pub enum DelegateCommands {
    /// 从 JSON 文件运行任务
    Run {
        ///包含任务定义的 JSON 文件 [{id, name, command, dir?}]
        file: PathBuf,
        ///最大并发任务数（0 = 不限）
        #[arg(short = 'c', long = "concurrent", default_value = "4")]
        max_concurrent: usize,
    },
    /// 直接运行命令（内联模式）
    Inline {
        /// 要并行运行的命令
        #[arg(required = true)]
        commands: Vec<String>,
        ///最大并发任务数（0 = 不限）
        #[arg(short = 'c', long = "concurrent", default_value = "4")]
        max_concurrent: usize,
    },
    /// 运行代理团队完成复杂目标
    Team {
        ///团队的高层目标
        goal: String,
        ///目标的可选上下文/背景
        #[arg(short = 'x', long = "context")]
        context: Option<String>,
        ///要包含的代理 ID 逗号分隔列表（默认：全部内置）
        #[arg(short = 'a', long = "agents")]
        agents: Option<String>,
        ///最大并行代理数（默认：4）
        #[arg(short = 'c', long = "concurrent", default_value = "4")]
        max_concurrent: usize,
    },
}

#[derive(Subcommand)]
/// Sop Commands.
pub enum SopCommands {
    /// 按 ID 执行 SOP
    Run { sop_id: String },
    /// 列出可用 SOP
    List,
}

#[derive(Subcommand)]
pub enum WorkspaceCommands {
    /// 创建新工作区
    Create {
        /// 工作区名
        name: String,
        /// 项目 ID
        project_id: String,
        /// 根目录
        root_dir: String,
    },
    /// 切换到工作区
    Switch {
        /// 工作区 ID
        workspace_id: String,
    },
    /// 列出所有工作区
    List,
    /// 显示当前工作区
    Current,
    /// 按 ID 删除工作区
    Delete {
        /// 工作区 ID
        workspace_id: String,
        ///跳过确认提示
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
}
