use clap::{Subcommand, ValueEnum};
use std::path::PathBuf;

// ── 子枚举分组模块 ──────────────────────────────

#[path = "cmd_agent.rs"]
mod cmd_agent;
pub use cmd_agent::*;

#[path = "cmd_git.rs"]
mod cmd_git;
pub use cmd_git::*;

#[path = "cmd_devtool.rs"]
mod cmd_devtool;
pub use cmd_devtool::*;

#[path = "cmd_bridge.rs"]
mod cmd_bridge;
pub use cmd_bridge::*;

// ── 主命令枚举 ──────────────────────────────────

#[derive(Subcommand)]
/// Commands.
pub enum Commands {
    // ── 文件工具 ──────────────────────────────────────
    #[command(next_help_heading = "File Tools")]
    ///读取文件并打印其内容
    Read { path: PathBuf },
    ///读取文件中的某段行（offset/limit）
    ReadLines {
        path: PathBuf,
        ///起始行偏移（从 0 开始）
        #[arg(short = 'o', long = "offset", default_value = "0")]
        offset: usize,
        ///要读取的行数
        #[arg(short = 'l', long = "limit", default_value = "50")]
        limit: usize,
    },
    ///将内容写入文件（原子创建 + 重命名）
    Write { path: PathBuf, content: String },
    ///编辑文件：将 OLD_STRING 的首次出现替换为 NEW_STRING
    Edit {
        path: PathBuf,
        old_string: String,
        new_string: String,
        ///替换所有出现，而不仅是第一次
        #[arg(short = 'a', long = "all")]
        all: bool,
    },
    ///查找匹配 glob 模式的文件
    Glob {
        pattern: String,
        ///搜索起始根目录（默认：.）
        #[arg(default_value = ".")]
        root: PathBuf,
        ///最大结果数（默认：100）
        #[arg(short = 'l', default_value = "100")]
        limit: usize,
    },
    ///跨文件正则搜索（grep）
    Grep {
        pattern: String,
        ///要搜索的文件或目录
        path: PathBuf,
        #[arg(short = 'i')]
        include: Option<String>,
    },
    ///执行 shell 命令
    Exec {
        command: String,
        #[arg(short = 'd', long = "dir")]
        dir: Option<PathBuf>,
    },
    ///使用 DuckDuckGo 搜索网络
    Search {
        query: String,
        #[arg(short = 'n', default_value = "5")]
        num: usize,
    },
    ///对文件应用 unified-diff 补丁
    ApplyPatch {
        ///要修补的文件
        file: PathBuf,
        ///补丁文件路径（使用 - 表示 stdin）
        #[arg(short = 'i', default_value = "-")]
        input: PathBuf,
    },

    #[command(next_help_heading = "Task Tracking", subcommand, alias = "td")]
    ///管理待办事项
    Todo(TodoCommands),

    ///管理会话
    #[command(subcommand, alias = "ses")]
    Session(SessionCommands),

    ///管理计划
    #[command(subcommand)]
    Plan(PlanCommands),

    #[command(next_help_heading = "Agents", subcommand)]
    ///代码分析工具
    Oracle(OracleCommands),

    ///代码搜索工具
    #[command(subcommand)]
    Explore(ExploreCommands),

    ///外部研究代理（网络搜索、文档、代码示例）
    #[command(subcommand)]
    Librarian(LibrarianCommands),

    ///计划/代码/架构审查代理
    #[command(subcommand)]
    Momus(MomusCommands),

    ///规划前分析代理（歧义检测、隐含需求）
    #[command(subcommand)]
    Metis(MetisCommands),

    ///技能管理（列出/查看/安装技能）
    #[command(subcommand)]
    Skill(SkillCommands),

    ///项目规则（AGENTS.md 加载与检查）
    #[command(subcommand)]
    Rules(RulesCommands),

    ///语言服务器协议工具
    #[command(subcommand)]
    Lsp(LspCommands),

    ///代理团队（运行多代理协作完成复杂目标）
    #[command(next_help_heading = "Agents", subcommand)]
    Team(TeamCommands),

    ///任务编排（并行执行）
    #[command(subcommand)]
    Delegate(DelegateCommands),

    #[command(next_help_heading = "Engine", subcommand)]
    ///MCP 服务器集成（连接、信息、调用）
    Mcp(McpCommands),

    // ── 引擎 ──────────────────────────────────────────
    ///通过 gRPC 桥接与 Sisyphus 认知引擎交互
    #[command(subcommand)]
    Sop(SopCommands),
    ///插件管理（列出、加载、监视、卸载）
    #[command(subcommand)]
    Plugin(PluginCommands),
    ///工作区管理（创建、切换、列出、当前、删除）
    #[command(subcommand)]
    Workspace(WorkspaceCommands),
    ///检查桥接健康状态
    Health,
    ///显示桥接状态（熔断器、基准）
    Status,
    ///显示来自桥接的元学习洞察
    LearningInsights {
        ///可选：按策略名过滤
        strategy: Option<String>,
    },
    ///与远程桥接资源交互
    #[command(subcommand)]
    Remote(RemoteCommands),
    ///启动桥接守护进程
    Daemon,
    ///运行单机环境与连接诊断（本地 + bridge gRPC/HTTP 可选）
    Doctor {
        ///可选：bridge HTTP 基地址（如 http://127.0.0.1:8080），用于检查 /health 等端点
        #[arg(long = "http")]
        http: Option<String>,
    },
    ///显示单机快速上手与常用流程指引
    Quickstart,

    #[command(next_help_heading = "Git", subcommand)]
    ///Git 操作（status、diff、add、commit、log、branch、checkout、push、pull）
    Git(GitCommands),

    #[command(next_help_heading = "Dev Tools", subcommand)]
    ///构建工具（check、build、test、lint、fmt、doc）
    Build(BuildCommands),

    // ── 运行 ────────────────────────────────────────────────
    ///运行项目（cargo run、npm start 等）
    #[command(subcommand)]
    Run(RunCommands),

    // ── 初始化 ──────────────────────────────────────────────
    ///项目脚手架：从模板生成项目
    #[command(subcommand)]
    Init(InitCommands),

    // ── 预提交 ─────────────────────────────────────────
    ///安装/卸载/检查 git pre-commit 钩子（fmt + lint）
    #[command(subcommand)]
    Precommit(PrecommitCommands),
    // ── pr ──────────────────────────────────────────────────
    /// 管理 GitHub 拉取请求
    #[command(subcommand)]
    Pr(PrCommands),
    // ── 智慧笔记 ──────────────────────────────────────────────
    ///管理跨会话知识（智慧/记事本）
    #[command(subcommand)]
    Wisdom(WisdomCommands),
    // ── 行哈希编辑 ────────────────────────────────────────────
    ///行哈希编辑安全系统（LINE#ID 模式）
    #[command(subcommand)]
    Hashline(HashlineCommands),
    // ── 诊断 ──────────────────────────────────────────
    ///运行 cargo check（可选 clippy），显示统一报告

    #[command(subcommand)]
    Diagnose(DiagnoseCommands),

    ///自动应用 cargo fix，然后重新诊断
    #[command(subcommand)]
    Fix(FixCommands),

    ///权限规则与审计（检查判定、查看审计日志）
    #[command(subcommand)]
    Permission(PermissionCommands),

    ///自定义开发命令（.sisyphus/commands/*.md：列出、查看、渲染/执行）
    #[command(subcommand, alias = "cmd")]
    Command(CommandCommands),

    ///验证（check + lint），可选有界自动修复（verify → fix → verify）
    Verify {
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///要验证的包
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        ///验证全部工作区成员
        #[arg(long = "all")]
        all: bool,
        ///跳过 lint（仅运行 check）
        #[arg(long = "no-lint", alias = "no-clippy")]
        no_lint: bool,
        ///未通过时自动修复并重试
        #[arg(long = "fix")]
        fix: bool,
        ///修复时同时应用 lint 修复
        #[arg(long = "lint-fixes", alias = "clippy")]
        lint_fixes: bool,
        ///最大验证轮数（默认 3，仅在 --fix 时生效）
        #[arg(long = "attempts", default_value_t = 3)]
        attempts: usize,
        ///允许的最大警告数（0 = 零警告门禁；省略表示不限制）
        #[arg(long = "max-warnings")]
        max_warnings: Option<usize>,
        ///输出机器可读 JSON
        #[arg(long = "json")]
        json: bool,
        ///把本次执行落成一条任务级质量记录（T4.1，供 `sisyphus metrics quality` 聚合）
        #[arg(long = "record")]
        record: Option<PathBuf>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },

    ///质量指标（任务级成功率 / 修复率 / 回归率聚合）
    #[command(subcommand)]
    Metrics(MetricsCommands),

    ///GitHub Actions 集成（工作流模板、CI 模式）
    #[command(subcommand)]
    Github(GithubCommands),

    ///管理 LLM 提供商（列出、添加、移除、测试）
    #[command(subcommand)]
    Provider(ProviderCommands),

    ///管理工具注册（MCP、自定义工具）
    #[command(subcommand)]
    Tool(ToolCommands),

    // ── 开发 ─────────────────────────────────────────────────
    ///自主开发代理：计划 → 实现 → 验证
    Dev {
        ///开发目标（要构建/更改的内容）
        goal: String,
        ///可选项目上下文（当前状态、约束）
        #[arg(short = 'c', long = "context")]
        context: Option<String>,
        ///并行执行步骤（最多 4 个并发）
        #[arg(short = 'p', long = "parallel")]
        parallel: bool,
        ///从最后检查点恢复，跳过已完成步骤
        #[arg(short = 'r', long = "resume")]
        resume: bool,
        ///整体超时（秒）；本地/慢速模型需要更长（默认 300）
        #[arg(long = "timeout-secs", default_value_t = 300)]
        timeout_secs: u64,
        ///客观验收命令（例如 `cargo clippy -- -D warnings`）。
        ///步骤自带的验证由模型自选，可能与目标不一致；该命令是唯一的成功判据：
        ///未通过则整体判失败，并带着输出自动修复（次数见 --verify-fix-attempts）
        #[arg(long = "verify-cmd")]
        verify_cmd: Option<String>,
        ///验收命令失败后的自动修复尝试次数（0 = 只判定不修复，默认 2）
        #[arg(long = "verify-fix-attempts", default_value_t = 2)]
        verify_fix_attempts: usize,
        ///把本次闭环落成一条任务级质量记录（T4.1）；必须同时指定 --verify-cmd。
        ///闸门失败也会落记录（失败样本是成功率的分母）
        #[arg(long = "record")]
        record: Option<std::path::PathBuf>,
        ///自动放行需要人工确认的命令（默认：交互式询问；非交互环境直接拒绝）。
        ///显式 deny 的权限规则仍然拒绝，--yes 不会绕过它
        #[arg(long = "yes", alias = "auto-approve")]
        yes: bool,
        ///任务记录的场景标签（默认 dev-closed-loop）
        #[arg(long = "scenario", default_value = "dev-closed-loop")]
        scenario: String,
    },
}

// ── 共享类型 ────────────────────────────────────────

#[derive(ValueEnum, Clone, Debug)]
/// Priority Arg.
pub enum PriorityArg {
    Low,
    Medium,
    High,
}

impl From<PriorityArg> for sisyphus_cli::tools::TodoPriority {
    fn from(p: PriorityArg) -> Self {
        match p {
            PriorityArg::Low => sisyphus_cli::tools::TodoPriority::Low,
            PriorityArg::Medium => sisyphus_cli::tools::TodoPriority::Medium,
            PriorityArg::High => sisyphus_cli::tools::TodoPriority::High,
        }
    }
}

// ── 表格格式化辅助 ────────────────────────────

/// Short id.
pub fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        id.to_string()
    } else {
        format!("{}…", &id[..12])
    }
}

/// Print table.
pub fn print_table(header: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        return;
    }
    let mut widths: Vec<usize> = header.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let hdr: Vec<String> = header
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{:width$}", h, width = widths[i]))
        .collect();
    println!("  {}", hdr.join("  "));
    let sep: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
    println!("  {}", sep.join("──"));
    for row in rows {
        let line: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:width$}", c, width = widths[i]))
            .collect();
        println!("  {}", line.join("  "));
    }
}

/// 质量指标子命令（T4.1）。
#[derive(Subcommand)]
pub enum MetricsCommands {
    ///从任务记录聚合质量指标（成功率 / 修复率 / 回归率）
    Quality {
        ///任务记录目录（每个 .json 文件一条记录）
        #[arg(
            long = "records",
            default_value = "docs/benchmarks/quality-metrics/records"
        )]
        records: PathBuf,
        ///输出机器可读 JSON
        #[arg(long = "json")]
        json: bool,
    },
}
