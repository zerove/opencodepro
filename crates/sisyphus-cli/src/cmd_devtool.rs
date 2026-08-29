use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
/// Github Commands.
pub enum GithubCommands {
    /// 生成 GitHub Actions 工作流 YAML 模板
    Action {
        ///工作流类型：ci、pr-review、release
        kind: String,
        ///输出文件路径（默认：.github/workflows/sisyphus-{kind}.yml）
        #[arg(short = 'o', long = "output")]
        output: Option<std::path::PathBuf>,
        ///Rust 工具链（默认：stable）
        #[arg(long = "toolchain", default_value = "stable")]
        toolchain: String,
        ///额外的 cargo 参数
        #[arg(long = "cargo-flags", default_value = "")]
        cargo_flags: String,
        ///排除的包（逗号分隔）
        #[arg(long = "exclude", default_value = "")]
        exclude: String,
        ///在 CI 中包括 clippy
        #[arg(long = "clippy")]
        clippy: bool,
        ///包括 cargo-deny 审计
        #[arg(long = "deny")]
        deny: bool,
    },
    /// 显示来自环境变量的 CI 模式配置
    CiConfig,
}

#[derive(Subcommand)]
pub enum ProviderCommands {
    /// 列出配置中注册的所有提供商
    List,
    /// 用提示词测试指定提供商
    Test {
        ///要测试的提供商名
        name: String,
        ///可选提示词（默认："Say hello in one sentence"）
        #[arg(default_value = "Say hello in one sentence")]
        prompt: String,
    },
    /// 显示提供商回退链顺序
    Fallback,
}

#[derive(Subcommand)]
pub enum ToolCommands {
    /// 将 MCP 服务器注册为工具
    RegisterMcp {
        ///工具前缀（例如 "filesystem"、"web"）
        prefix: String,
        ///启动 MCP 服务器的命令（例如 "npx @modelcontextprotocol/server-filesystem /tmp"）
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// 列出配置中注册的所有工具
    List,
}

#[derive(Subcommand)]
pub enum HashlineCommands {
    ///为文件所有非空行添加 LINE#ID 注释：`hashline annotate <file>`
    Annotate { path: PathBuf },
    ///验证文件中所有 LINE#ID 仍然有效：`hashline verify <file>`
    Verify { path: PathBuf },
    ///使用 LINE#ID 编辑一行：`hashline edit <file> <hashline> <new-content>`
    Edit {
        path: PathBuf,
        hashline: String,
        new_content: String,
    },
}

#[derive(Subcommand)]
/// Pr Commands.
pub enum PrCommands {
    /// 列出打开的拉取请求
    List,
    ///检出拉取请求分支：`pr checkout <number>`
    Checkout {
        /// PR 编号
        number: u64,
    },
    ///对拉取请求运行 LLM 审查：`pr review <number>`
    Review {
        /// PR 编号
        number: u64,
        ///通过 `gh pr comment` 将审查作为 PR 评论发布
        #[arg(long)]
        post: bool,
        ///输出格式：plain（默认）或 json
        #[arg(long, default_value = "plain")]
        format: String,
    },
}

#[derive(Subcommand)]
/// Wisdom Commands.
pub enum WisdomCommands {
    /// 记录一条智慧：`wisdom record <category> <content>`
    Record {
        ///类别（例如 conventions、successes、failures、patterns、experience）
        category: String,
        ///要记录的内容
        content: String,
    },
    /// 列出所有可用的智慧类别
    List,
    ///显示特定类别的内容：`wisdom show <category>`
    Show {
        /// 类别名
        category: String,
    },
    ///删除智慧类别：`wisdom delete <category>`
    Delete {
        /// 类别名
        category: String,
    },
    /// 将所有智慧注入系统提示词
    Inject,
}

#[derive(Subcommand)]
pub enum RemoteCommands {
    /// 列出认知循环基准
    Bench,
    /// 列出已注册代理
    Agents,
    /// 列出已注册工具
    Tools,
    /// 列出已安装技能
    Skills,
    /// 按 ID 执行 SOP
    Exec { sop_id: String },
    /// 将目标分解为任务 DAG
    Decompose { goal: String },
}

#[derive(Subcommand)]
pub enum PluginCommands {
    /// 列出所有已安装插件
    List,
    /// 从路径加载插件
    Load { path: PathBuf },
    /// 监视目录中的插件变更
    Watch {
        ///要监视的目录（默认：tools 目录）
        path: Option<PathBuf>,
    },
    /// 按名称卸载插件
    Unload { name: String },
}

#[derive(Subcommand)]
pub enum DiagnoseCommands {
    /// 运行 check + lint，显示统一报告
    Run {
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///要诊断的包
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        ///诊断全部工作区成员
        #[arg(long = "all")]
        all: bool,
        ///跳过 lint（仅运行 check）
        #[arg(long = "no-lint", alias = "no-clippy")]
        no_lint: bool,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
/// Fix Commands.
pub enum FixCommands {
    /// 自动应用修复，然后重新诊断
    Run {
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        /// 要修复的包
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        ///同时应用 lint 修复
        #[arg(long = "lint-fixes", alias = "clippy")]
        lint_fixes: bool,
        ///修复全部工作区成员
        #[arg(long = "all")]
        all: bool,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
/// Init Commands.
pub enum InitCommands {
    /// 列出可用项目模板
    List,
    /// 从模板生成新项目
    Generate {
        ///模板名（rust-lib、rust-bin、rust-workspace）
        template: String,
        ///项目名
        name: String,
        ///输出目录（默认：当前目录）
        #[arg(short = 'd', long = "dir", default_value = ".")]
        dir: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
/// Precommit Commands.
pub enum PrecommitCommands {
    /// 安装 pre-commit 钩子（fmt + lint）
    Install {
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        dir: Option<std::path::PathBuf>,
        ///跳过格式检查
        #[arg(long = "no-fmt")]
        no_fmt: bool,
        ///跳过 lint 检查
        #[arg(long = "no-lint")]
        no_lint: bool,
        ///允许警告（不视为错误）
        #[arg(long = "allow-warnings")]
        allow_warnings: bool,
        ///语言后端（rust/node/python/go/java/gradle/fe，默认：rust）
        #[arg(long = "lang", default_value = "rust")]
        lang: Option<String>,
    },
    /// 移除 pre-commit 钩子
    Uninstall {
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        dir: Option<std::path::PathBuf>,
    },
    /// 显示 pre-commit 钩子状态
    Status {
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        dir: Option<std::path::PathBuf>,
    },
}
