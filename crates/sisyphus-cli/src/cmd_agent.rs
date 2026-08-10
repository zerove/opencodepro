use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum OracleCommands {
    ///分析源文件（函数、导入、行数）
    AnalyzeFile { path: PathBuf },
    ///分析目录（模块摘要）
    AnalyzeDir {
        path: PathBuf,
        #[arg(short = 't')]
        include: Option<String>,
    },
    ///审查代码中的潜在问题
    CodeReview { paths: Vec<PathBuf> },
}

#[derive(Subcommand)]
pub enum ExploreCommands {
    ///使用正则模式搜索文件内容
    Search {
        patterns: Vec<String>,
        #[arg(short = 'p')]
        paths: Vec<PathBuf>,
        #[arg(short = 'm', default_value = "50")]
        max_matches: usize,
        #[arg(short = 'c', default_value = "2")]
        context_lines: usize,
        #[arg(short = 'i')]
        include: Option<String>,
    },
    ///按 glob 模式查找文件
    FindFiles {
        pattern: String,
        root: PathBuf,
        #[arg(short = 'l', default_value = "100")]
        limit: usize,
    },
}

#[derive(Subcommand)]
pub enum LibrarianCommands {
    ///搜索网络并综合发现
    Research {
        query: String,
        ///研究范围：web、docs、code、all
        #[arg(short = 's', long = "scope", default_value = "all")]
        scope: String,
        ///最大结果数
        #[arg(short = 'n', default_value = "5")]
        num: usize,
    },
    ///从 URL 获取并显示文档
    FetchDocs { url: String },
    ///搜索库/API 的代码示例
    Examples {
        query: String,
        #[arg(short = 'l', default_value = "rust")]
        language: String,
    },
    ///从 JSON 研究报告综合发现
    Synthesise { report_file: std::path::PathBuf },
}

#[derive(Subcommand)]
#[allow(
    clippy::enum_variant_names,
    reason = "all Momus commands start with Review for naming consistency"
)]
pub enum MomusCommands {
    ///审查计划文件的完整性与一致性
    ReviewPlan { plan_file: std::path::PathBuf },
    ///审查源代码文件中的常见问题
    ReviewCode { path: std::path::PathBuf },
    ///审查架构文档
    ReviewArch { text: String },
    ///审查目录中的所有 Rust 文件
    ReviewDir { path: std::path::PathBuf },
}

#[derive(Subcommand)]
pub enum MetisCommands {
    ///分析请求中的歧义与隐含需求
    Analyse {
        ///请求文本（内联）
        request: Vec<String>,
        ///从文件读取请求
        #[arg(short = 'f', long = "file")]
        file: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum SkillCommands {
    ///列出所有可用技能（来自所有来源）
    List,
    ///显示特定技能的详细信息
    Show { name: String },
    ///将 .md 文件中的技能安装到用户目录
    Install { path: PathBuf },
    ///移除已安装的技能
    Remove { name: String },
}

#[derive(Subcommand)]
pub enum RulesCommands {
    ///显示已加载的项目规则（来自 AGENTS.md）
    Show,
    ///从 AGENTS.md 重新加载项目规则
    Reload,
}

#[derive(Subcommand)]
pub enum TeamCommands {
    ///运行代理团队完成复杂目标
    Run {
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
