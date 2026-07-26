use clap::{Subcommand, ValueEnum};
use std::path::PathBuf;

// ── Sub-enum group modules ──────────────────────────────

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

// ── Main Commands enum ──────────────────────────────────

#[derive(Subcommand)]
pub enum Commands {
    // ── file tools ──────────────────────────────────────
    #[command(next_help_heading = "File Tools")]
    /// Read a file and print its contents
    Read { path: PathBuf },
    /// Read a range of lines from a file (offset/limit)
    ReadLines {
        path: PathBuf,
        /// 0-based line offset to start from
        #[arg(short = 'o', long = "offset", default_value = "0")]
        offset: usize,
        /// Number of lines to read
        #[arg(short = 'l', long = "limit", default_value = "50")]
        limit: usize,
    },
    /// Write content to a file (atomic create + rename)
    Write { path: PathBuf, content: String },
    /// Edit a file: replace FIRST occurrence of OLD_STRING with NEW_STRING
    Edit {
        path: PathBuf,
        old_string: String,
        new_string: String,
        /// Replace ALL occurrences instead of just the first
        #[arg(short = 'a', long = "all")]
        all: bool,
    },
    /// Find files matching a glob pattern
    Glob {
        pattern: String,
        /// Root directory to search from (default: .)
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Max results (default: 100)
        #[arg(short = 'l', default_value = "100")]
        limit: usize,
    },
    /// Regex search across files (grep)
    Grep {
        pattern: String,
        /// File or directory to search
        path: PathBuf,
        #[arg(short = 'i')]
        include: Option<String>,
    },
    /// Execute a shell command
    Exec {
        command: String,
        #[arg(short = 'd', long = "dir")]
        dir: Option<PathBuf>,
    },
    /// Search the web using DuckDuckGo
    Search {
        query: String,
        #[arg(short = 'n', default_value = "5")]
        num: usize,
    },
    /// Apply a unified-diff patch to a file
    ApplyPatch {
        /// File to patch
        file: PathBuf,
        /// Path to a patch file (use - for stdin)
        #[arg(short = 'i', default_value = "-")]
        input: PathBuf,
    },

    #[command(next_help_heading = "Task Tracking", subcommand, alias = "td")]
    /// Manage todos
    Todo(TodoCommands),

    /// Manage sessions
    #[command(subcommand, alias = "ses")]
    Session(SessionCommands),

    /// Manage plans
    #[command(subcommand)]
    Plan(PlanCommands),

    #[command(next_help_heading = "Agents", subcommand)]
    /// Code analysis tools
    Oracle(OracleCommands),

    /// Code search tools
    #[command(subcommand)]
    Explore(ExploreCommands),

    /// External research agent (web search, docs, code examples)
    #[command(subcommand)]
    Librarian(LibrarianCommands),

    /// Plan/code/architecture review agent
    #[command(subcommand)]
    Momus(MomusCommands),

    /// Pre-planning analysis agent (ambiguity detection, implicit requirements)
    #[command(subcommand)]
    Metis(MetisCommands),

    /// Skill management (list/show/install skills)
    #[command(subcommand)]
    Skill(SkillCommands),

    /// Project rules (AGENTS.md loading and inspection)
    #[command(subcommand)]
    Rules(RulesCommands),

    /// Language server protocol tools
    #[command(subcommand)]
    Lsp(LspCommands),

    /// Task orchestration (parallel execution)
    #[command(subcommand)]
    Delegate(DelegateCommands),

    #[command(next_help_heading = "Engine", subcommand)]
    /// MCP server integration (connect, info, call)
    Mcp(McpCommands),

    // ── engine ──────────────────────────────────────────
    /// Interact with the Sisyphus cognitive engine via gRPC bridge
    #[command(subcommand)]
    Sop(SopCommands),
    /// Plugin management (list, load, watch, unload)
    #[command(subcommand)]
    Plugin(PluginCommands),
    /// Workspace management (create, switch, list, current, delete)
    #[command(subcommand)]
    Workspace(WorkspaceCommands),
    /// Check bridge health
    Health,
    /// Show bridge status (breakers, benchmarks)
    Status,
    /// Show meta-learning insights from the bridge
    LearningInsights {
        /// Optional: filter by strategy name
        strategy: Option<String>,
    },
    /// Interact with remote bridge resources
    #[command(subcommand)]
    Remote(RemoteCommands),
    /// Start the bridge daemon
    Daemon,

    #[command(next_help_heading = "Git", subcommand)]
    /// Git operations (status, diff, add, commit, log, branch, checkout, push, pull)
    Git(GitCommands),

    #[command(next_help_heading = "Dev Tools", subcommand)]
    /// Build tools (check, build, test, lint, fmt, doc)
    Build(BuildCommands),

    // ── run ────────────────────────────────────────────────
    /// Run the project (cargo run, npm start, etc.)
    #[command(subcommand)]
    Run(RunCommands),

    // ── init ──────────────────────────────────────────────
    /// Project scaffolding: generate project from templates
    #[command(subcommand)]
    Init(InitCommands),

    // ── precommit ─────────────────────────────────────────
    /// Install/uninstall/check git pre-commit hooks (fmt + lint)
    #[command(subcommand)]
    Precommit(PrecommitCommands),
    // ── pr ──────────────────────────────────────────────────
    /// Manage GitHub Pull Requests
    #[command(subcommand)]
    Pr(PrCommands),
    // ── wisdom ──────────────────────────────────────────────
    /// Manage wisdom/notepads for cross-session knowledge
    #[command(subcommand)]
    Wisdom(WisdomCommands),
    // ── hashline ────────────────────────────────────────────
    /// Hashline editing safety system (LINE#ID pattern)
    #[command(subcommand)]
    Hashline(HashlineCommands),
    // ── diagnose ──────────────────────────────────────────
    /// Run cargo check (and optionally clippy), show unified report

    #[command(subcommand)]
    Diagnose(DiagnoseCommands),

    /// Auto-apply cargo fix, then re-diagnose
    #[command(subcommand)]
    Fix(FixCommands),

    /// GitHub Actions integration (workflow templates, CI mode)
    #[command(subcommand)]
    Github(GithubCommands),

    /// Manage LLM providers (list, add, remove, test)
    #[command(subcommand)]
    Provider(ProviderCommands),

    /// Manage tool registration (MCP, custom tools)
    #[command(subcommand)]
    Tool(ToolCommands),

    // ── dev ─────────────────────────────────────────────────
    /// Autonomous development agent: plan → implement → verify
    Dev {
        /// Development goal (what to build/change)
        goal: String,
        /// Optional project context (current state, constraints)
        #[arg(short = 'c', long = "context")]
        context: Option<String>,
        /// Execute steps in parallel (max 4 concurrent)
        #[arg(short = 'p', long = "parallel")]
        parallel: bool,
        /// Resume from last checkpoint, skipping completed steps
        #[arg(short = 'r', long = "resume")]
        resume: bool,
    },
}

// ── Shared types ────────────────────────────────────────

#[derive(ValueEnum, Clone, Debug)]
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

// ── Table formatting helpers ────────────────────────────

pub fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        id.to_string()
    } else {
        format!("{}…", &id[..12])
    }
}

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
