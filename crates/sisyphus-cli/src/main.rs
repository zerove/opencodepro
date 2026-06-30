use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use sisyphus_cli::*;

#[derive(Parser)]
#[command(name = "sisyphus", version, about = "Sisyphus Cognitive Engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable interactive TUI mode
    #[arg(short = 'i', long = "interactive")]
    interactive: bool,

    /// Bridge address (default: `http://[::1]:50051`)
    #[arg(short = 'b', long = "bridge", env = "SISYPHUS_BRIDGE_ADDRESS")]
    bridge: Option<String>,

    /// Data directory for todo/session storage
    #[arg(long = "data-dir", env = "SISYPHUS_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Path to Sisyphus TOML config file
    #[arg(long = "config", env = "SISYPHUS_CONFIG")]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
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
}

#[derive(Subcommand)]
enum GithubCommands {
    /// Generate a GitHub Actions workflow YAML template
    Action {
        /// Workflow kind: ci, pr-review, release
        kind: String,
        /// Output file path (default: .github/workflows/sisyphus-{kind}.yml)
        #[arg(short = 'o', long = "output")]
        output: Option<std::path::PathBuf>,
        /// Rust toolchain (default: stable)
        #[arg(long = "toolchain", default_value = "stable")]
        toolchain: String,
        /// Extra cargo flags
        #[arg(long = "cargo-flags", default_value = "")]
        cargo_flags: String,
        /// Exclude packages (comma-separated)
        #[arg(long = "exclude", default_value = "")]
        exclude: String,
        /// Include clippy in CI
        #[arg(long = "clippy")]
        clippy: bool,
        /// Include cargo-deny audit
        #[arg(long = "deny")]
        deny: bool,
    },
    /// Show CI mode configuration from environment
    CiConfig,
}

#[derive(Subcommand)]
enum ProviderCommands {
    /// List all registered providers from config
    List,
    /// Test a provider by name with a prompt
    Test {
        /// Provider name to test
        name: String,
        /// Optional prompt (default: "Say hello in one sentence")
        #[arg(default_value = "Say hello in one sentence")]
        prompt: String,
    },
    /// Show provider fallback chain order
    Fallback,
}

#[derive(Subcommand)]
enum ToolCommands {
    /// Register an MCP server as a tool
    RegisterMcp {
        /// Tool prefix (e.g. "filesystem", "web")
        prefix: String,
        /// Command to launch the MCP server (e.g. "npx @modelcontextprotocol/server-filesystem /tmp")
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// List all registered tools from config
    List,
}

#[derive(Subcommand)]
enum HashlineCommands {
    /// Add LINE#ID comments to all non-empty lines in a file: `hashline annotate <file>`
    Annotate { path: PathBuf },
    /// Verify all LINE#IDs in a file are still valid: `hashline verify <file>`
    Verify { path: PathBuf },
    /// Edit a line using its LINE#ID: `hashline edit <file> <hashline> <new-content>`
    Edit {
        path: PathBuf,
        hashline: String,
        new_content: String,
    },
}

#[derive(Subcommand)]
enum PrCommands {
    /// List open pull requests
    List,
    /// Checkout a pull request branch: `pr checkout <number>`
    Checkout {
        /// PR number
        number: u64,
    },
    /// Run an LLM review of a pull request: `pr review <number>`
    Review {
        /// PR number
        number: u64,
        /// Post the review as a PR comment via `gh pr comment`
        #[arg(long)]
        post: bool,
        /// Output format: plain (default) or json
        #[arg(long, default_value = "plain")]
        format: String,
    },
}

#[derive(Subcommand)]
enum WisdomCommands {
    /// Record a piece of wisdom: `wisdom record <category> <content>`
    Record {
        /// Category (e.g. conventions, successes, failures, patterns, experience)
        category: String,
        /// Content to record
        content: String,
    },
    /// List all available wisdom categories
    List,
    /// Show content of a specific category: `wisdom show <category>`
    Show {
        /// Category name
        category: String,
    },
    /// Delete a wisdom category: `wisdom delete <category>`
    Delete {
        /// Category name
        category: String,
    },
    /// Inject all wisdom into the system prompt
    Inject,
}

#[derive(Subcommand)]
enum RemoteCommands {
    /// List cognitive loop benchmarks
    Bench,
    /// List registered agents
    Agents,
    /// List registered tools
    Tools,
    /// List installed skills
    Skills,
    /// Execute an SOP by ID
    Exec { sop_id: String },
    /// Decompose a goal into a task DAG
    Decompose { goal: String },
}

#[derive(Subcommand)]
enum PluginCommands {
    /// List all installed plugins
    List,
    /// Load a plugin from a path
    Load { path: PathBuf },
    /// Watch a directory for plugin changes
    Watch {
        /// Directory to watch (default: tools dir)
        path: Option<PathBuf>,
    },
    /// Unload a plugin by name
    Unload { name: String },
}

#[derive(Subcommand)]
enum DiagnoseCommands {
    /// Run check + lint, show unified report
    Run {
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Package to diagnose
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Diagnose all workspace members
        #[arg(long = "all")]
        all: bool,
        /// Skip lint (only run check)
        #[arg(long = "no-lint", alias = "no-clippy")]
        no_lint: bool,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum FixCommands {
    /// Auto-apply fixes, then re-diagnose
    Run {
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Package to fix
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Also apply lint fixes
        #[arg(long = "lint-fixes", alias = "clippy")]
        lint_fixes: bool,
        /// Fix all workspace members
        #[arg(long = "all")]
        all: bool,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum InitCommands {
    /// List available project templates
    List,
    /// Generate a new project from a template
    Generate {
        /// Template name (rust-lib, rust-bin, rust-workspace)
        template: String,
        /// Project name
        name: String,
        /// Output directory (default: current dir)
        #[arg(short = 'd', long = "dir", default_value = ".")]
        dir: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
enum PrecommitCommands {
    /// Install a pre-commit hook (fmt + lint)
    Install {
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        dir: Option<std::path::PathBuf>,
        /// Skip the format check
        #[arg(long = "no-fmt")]
        no_fmt: bool,
        /// Skip the lint check
        #[arg(long = "no-lint")]
        no_lint: bool,
        /// Allow warnings (don't deny)
        #[arg(long = "allow-warnings")]
        allow_warnings: bool,
        /// Language backend (rust/node/python/go/java/gradle/fe, default: rust)
        #[arg(long = "lang", default_value = "rust")]
        lang: Option<String>,
    },
    /// Remove the pre-commit hook
    Uninstall {
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        dir: Option<std::path::PathBuf>,
    },
    /// Show pre-commit hook status
    Status {
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        dir: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum GitCommands {
    /// Show working tree status
    Status {
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Show diff of working tree or staged changes
    Diff {
        /// Optional pathspec to filter
        pathspec: Option<String>,
        /// Show staged (cached) diff
        #[arg(short = 'c', long = "cached")]
        cached: bool,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Stage files
    Add {
        /// Paths to stage (use "." for all)
        pathspecs: Vec<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Create a commit
    Commit {
        /// Commit message
        #[arg(short = 'm', long = "message")]
        message: String,
        /// Allow empty commit
        #[arg(long = "allow-empty")]
        allow_empty: bool,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Show commit log
    Log {
        /// Number of commits to show
        #[arg(short = 'n', default_value = "10")]
        count: usize,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// List branches
    Branch {
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Switch to a branch (or create-and-switch with -b)
    Checkout {
        /// Branch name
        branch: String,
        /// Create the branch and switch to it
        #[arg(short = 'b')]
        create: bool,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Push to remote
    Push {
        /// Remote name (default: origin)
        #[arg(default_value = "origin")]
        remote: String,
        /// Branch to push (default: current branch)
        branch: Option<String>,
        /// Force push
        #[arg(short = 'f', long = "force")]
        force: bool,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Pull from remote
    Pull {
        /// Remote name (default: origin)
        #[arg(default_value = "origin")]
        remote: String,
        /// Branch to pull (default: current branch)
        branch: Option<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Show a one-line summary (branch + counts)
    Summary {
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum BuildCommands {
    /// Check code for errors
    Check {
        /// Package to check
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Check all workspace members
        #[arg(long = "all")]
        all: bool,
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Build the project
    Build {
        /// Package to build
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Build in release mode
        #[arg(short = 'r', long = "release")]
        release: bool,
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Run tests
    Test {
        /// Run only the test whose name matches `<filter>`
        #[arg(short = 'n', long = "name")]
        name: Option<String>,
        /// Package to test
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Test all workspace members
        #[arg(long = "all")]
        all: bool,
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Lint code
    Lint {
        /// Package to lint
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Format code
    Fmt {
        /// Check formatting without modifying files
        #[arg(long = "check")]
        check: bool,
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// Generate documentation
    Doc {
        /// Package to document
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum RunCommands {
    /// Run the project (cargo run, npm start, go run ., etc.)
    Run {
        /// Extra arguments for the run command (passed after --)
        #[arg(last = true)]
        args: Vec<String>,
        /// Language backend (rust/node/python/go/java/gradle/fe). Auto-detected if omitted.
        #[arg(long = "lang")]
        lang: Option<String>,
        /// Working directory (default: current dir)
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
}

// ── Sub-command enums ────────────────────────────────────

#[derive(Subcommand)]
enum TodoCommands {
    /// Add a new todo item
    Add {
        content: String,
        #[arg(short = 'p', default_value = "medium")]
        priority: PriorityArg,
    },
    /// List todos, optionally filtered by status
    List {
        #[arg(short = 's')]
        status: Option<String>,
    },
    /// Update a todo
    Update {
        id: String,
        #[arg(short = 'c')]
        content: Option<String>,
        #[arg(short = 's')]
        status: Option<String>,
        #[arg(short = 'p')]
        priority: Option<PriorityArg>,
    },
    /// Delete a todo by ID
    Delete {
        id: String,
        /// Skip confirmation prompt
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// Create a new session
    Create { name: String },
    /// List all sessions
    List,
    /// Get a session by ID
    Get { id: String },
    /// Delete a session by ID
    Delete {
        id: String,
        /// Skip confirmation prompt
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// Add a message to a session
    AddMessage {
        id: String,
        role: String,
        content: String,
    },
    /// Rename a session
    Rename { id: String, name: String },
    /// Fork a session — create a new copy with all messages
    Fork { id: String, name: String },
    /// Continue a session — touch updated_at for ordering
    Continue { id: String },
    /// Undo the last message in a session
    Undo { id: String },
    /// Export a session to a file (json or html format)
    Export {
        id: String,
        output: PathBuf,
        /// Output format: json (default) or html
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Import a session from a JSON file
    Import { input: PathBuf },
    /// Compress a session based on token threshold
    Compress {
        id: String,
        /// Optional token threshold (default: 32000)
        threshold: Option<u64>,
    },
}

#[derive(Subcommand)]
enum PlanCommands {
    /// Create a plan from requirements text
    Create {
        name: String,
        #[arg(short = 'd', default_value = "")]
        description: String,
        /// Requirements file (lines starting with "- " become steps)
        #[arg(short = 'f')]
        file: Option<PathBuf>,
    },
    /// Save a plan to a JSON file
    Save { plan_file: PathBuf, output: PathBuf },
    /// Load and display a plan
    Show { plan_file: PathBuf },
    /// Review a plan for issues
    Review { plan_file: PathBuf },
    /// Approve a plan
    Approve { plan_file: PathBuf },
}

#[derive(Subcommand)]
enum OracleCommands {
    /// Analyze a source file (functions, imports, line count)
    AnalyzeFile { path: PathBuf },
    /// Analyze a directory (module summary)
    AnalyzeDir {
        path: PathBuf,
        #[arg(short = 't')]
        include: Option<String>,
    },
    /// Review code for potential issues
    CodeReview { paths: Vec<PathBuf> },
}

#[derive(Subcommand)]
enum ExploreCommands {
    /// Search file contents with regex patterns
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
    /// Find files by glob pattern
    FindFiles {
        pattern: String,
        root: PathBuf,
        #[arg(short = 'l', default_value = "100")]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum LibrarianCommands {
    /// Search the web and synthesise findings
    Research {
        query: String,
        /// Research scope: web, docs, code, all
        #[arg(short = 's', long = "scope", default_value = "all")]
        scope: String,
        /// Max results
        #[arg(short = 'n', default_value = "5")]
        num: usize,
    },
    /// Fetch and display documentation from a URL
    FetchDocs { url: String },
    /// Search for code examples for a library/API
    Examples {
        query: String,
        #[arg(short = 'l', default_value = "rust")]
        language: String,
    },
    /// Synthesise findings from a JSON research report
    Synthesise { report_file: std::path::PathBuf },
}

#[derive(Subcommand)]
#[allow(clippy::enum_variant_names)]
enum MomusCommands {
    /// Review a plan file for completeness and consistency
    ReviewPlan { plan_file: std::path::PathBuf },
    /// Review a source code file for common issues
    ReviewCode { path: std::path::PathBuf },
    /// Review architecture documentation
    ReviewArch { text: String },
    /// Review all Rust files in a directory
    ReviewDir { path: std::path::PathBuf },
}

#[derive(Subcommand)]
enum MetisCommands {
    /// Analyse a request for ambiguities and implicit requirements
    Analyse {
        /// Request text (inline)
        request: Vec<String>,
        /// Read request from a file
        #[arg(short = 'f', long = "file")]
        file: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum SkillCommands {
    /// List all available skills (from all sources)
    List,
    /// Show detailed info for a specific skill
    Show { name: String },
    /// Install a skill from a .md file into the user directory
    Install { path: PathBuf },
    /// Remove an installed skill
    Remove { name: String },
}

#[derive(Subcommand)]
enum RulesCommands {
    /// Show loaded project rules (from AGENTS.md)
    Show,
    /// Reload project rules from AGENTS.md
    Reload,
}

#[derive(Subcommand)]
enum LspCommands {
    /// Get diagnostics for a file
    Diagnostics {
        /// File path to analyze
        file: PathBuf,
        /// Language ID (e.g. "rust", "typescript", "python")
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        /// Language server command (e.g. "rust-analyzer")
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        /// Extra server args
        #[arg(last = true)]
        server_args: Vec<String>,
        /// Root URI for the workspace
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
    /// Go to definition of a symbol
    GotoDef {
        /// File path
        file: PathBuf,
        /// Line number (0-based)
        line: u32,
        /// Column number (0-based)
        col: u32,
        /// Language ID
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        /// Language server command
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        /// Extra server args
        #[arg(last = true)]
        server_args: Vec<String>,
        /// Root URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
    /// Find references to a symbol
    FindRefs {
        /// File path
        file: PathBuf,
        /// Line number (0-based)
        line: u32,
        /// Col number (0-based)
        col: u32,
        /// Language ID
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        /// Language server command
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        /// Extra server args
        #[arg(last = true)]
        server_args: Vec<String>,
        /// Root URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
    /// Get hover information at a position
    Hover {
        /// File path
        file: PathBuf,
        /// Line number (0-based)
        line: u32,
        /// Col number (0-based)
        col: u32,
        /// Language ID
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        /// Language server command
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        /// Extra server args
        #[arg(last = true)]
        server_args: Vec<String>,
        /// Root URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
    /// List document symbols
    Symbols {
        /// File path
        file: PathBuf,
        /// Language ID
        #[arg(short = 'l', long = "language", default_value = "rust")]
        language_id: String,
        /// Language server command
        #[arg(short = 's', long = "server", default_value = "rust-analyzer")]
        server: String,
        /// Extra server args
        #[arg(last = true)]
        server_args: Vec<String>,
        /// Root URI
        #[arg(short = 'r', long = "root", default_value = "file:///")]
        root_uri: String,
    },
}

#[derive(Subcommand)]
enum McpCommands {
    /// Connect to an MCP server and show info + available tools
    Info {
        /// Server command/URL followed by optional args.
        /// URL (http://...) → HTTP transport.
        /// Otherwise treated as `<command>` `[args...]` → stdio transport.
        #[arg(required = true)]
        connect: Vec<String>,
    },
    /// Call a tool on an MCP server
    Call {
        /// Name of the tool to call
        tool: String,
        /// JSON arguments (optional, e.g. '{"text":"hello"}')
        args: Option<String>,
        /// Server connection: `<url>` or `<command>` `[args...]`
        #[arg(required = true)]
        connect: Vec<String>,
    },
    /// Start an MCP server that exposes Sisyphus agent tools via stdio
    Serve,
}

#[derive(Subcommand)]
enum DelegateCommands {
    /// Run tasks from a JSON file
    Run {
        /// JSON file with task definitions [{id, name, command, dir?}]
        file: PathBuf,
        /// Max concurrent tasks (0 = unlimited)
        #[arg(short = 'c', long = "concurrent", default_value = "4")]
        max_concurrent: usize,
    },
    /// Run commands directly (inline mode)
    Inline {
        /// Commands to run in parallel
        #[arg(required = true)]
        commands: Vec<String>,
        /// Max concurrent tasks (0 = unlimited)
        #[arg(short = 'c', long = "concurrent", default_value = "4")]
        max_concurrent: usize,
    },
}

#[derive(Subcommand)]
enum SopCommands {
    /// Execute an SOP by ID
    Run { sop_id: String },
    /// List available SOPs
    List,
}

#[derive(ValueEnum, Clone, Debug)]
enum PriorityArg {
    Low,
    Medium,
    High,
}

impl From<PriorityArg> for sisyphus_tools::TodoPriority {
    fn from(p: PriorityArg) -> Self {
        match p {
            PriorityArg::Low => sisyphus_tools::TodoPriority::Low,
            PriorityArg::Medium => sisyphus_tools::TodoPriority::Medium,
            PriorityArg::High => sisyphus_tools::TodoPriority::High,
        }
    }
}

// ── Table formatting helpers ────────────────────────────

fn short_id(id: &str) -> String {
    if id.len() <= 12 { id.to_string() } else { format!("{}…", &id[..12]) }
}

fn print_table(header: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() { return; }
    let mut widths: Vec<usize> = header.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let hdr: Vec<String> = header.iter().enumerate()
        .map(|(i, h)| format!("{:width$}", h, width = widths[i]))
        .collect();
    println!("  {}", hdr.join("  "));
    let sep: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
    println!("  {}", sep.join("──"));
    for row in rows {
        let line: Vec<String> = row.iter().enumerate()
            .map(|(i, c)| format!("{:width$}", c, width = widths[i]))
            .collect();
        println!("  {}", line.join("  "));
    }
}

// ── Main ─────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let platform_info = platform::detect_platform();
    tracing::info!(
        "Starting Sisyphus CLI on {} (shell: {})",
        platform_info.os,
        platform_info.shell
    );

    let cli = Cli::parse();

    // Resolve config: try file, then env, then defaults
    let sisyphus_cfg = if let Some(ref config_path) = cli.config {
        match sisyphus_types::Config::from_file(&config_path.to_string_lossy()) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!(
                    "Warning: failed to load config '{}': {e}",
                    config_path.display()
                );
                sisyphus_types::Config::default()
            }
        }
    } else {
        sisyphus_types::Config::default()
    };

    let data_dir = cli.data_dir.clone().unwrap_or_else(|| {
        let sqlite = PathBuf::from(&sisyphus_cfg.cache.sqlite_path);
        sqlite
            .parent()
            .unwrap_or(Path::new(".sisyphus"))
            .to_path_buf()
    });
    let bridge_addr = cli
        .bridge
        .clone()
        .unwrap_or_else(|| sisyphus_cfg.bridge_address.clone());

    // Interactive TUI mode
    if cli.interactive || cli.command.is_none() {
        tui::run(data_dir, bridge_addr).await?;
        return Ok(());
    }

    // Ensure data directory exists
    std::fs::create_dir_all(&data_dir)?;

    let cli_perms = permission::CliPermission::load(None).unwrap_or_default();

    let todo_db = data_dir.join("todos.json");
    let session_db = data_dir.join("sessions.json");

    // Create LLM client from config for agent commands.
    // Falls back to DummyLlmClient if no endpoint is configured.
    let llm_client: std::sync::Arc<dyn sisyphus_tools::metis::LlmClient> = {
        let endpoint = sisyphus_cfg.llm.endpoint.clone();
        let is_default_endpoint = endpoint == "http://localhost:8000/v1/chat/completions";
        if is_default_endpoint {
            std::sync::Arc::new(sisyphus_tools::DummyLlmClient)
        } else {
            let llm_config = std::sync::Arc::new(std::sync::RwLock::new(sisyphus_cfg.clone()));
            std::sync::Arc::new(sisyphus_tools::llm::DynamicLlmClient::new(llm_config))
        }
    };

    match cli.command.as_ref().context("No command specified")? {
        // ── file tools ──────────────────────────────────
        Commands::Read { path } => {
            let content = sisyphus_tools::file::read(path)?;
            println!("{content}");
        }
        Commands::ReadLines {
            path,
            offset,
            limit,
        } => {
            let content = sisyphus_tools::file::read_lines(path, *offset, *limit)?;
            print!("{content}");
        }
        Commands::Write { path, content } => {
            sisyphus_tools::file::write(path, content)?;
            println!("Written {} bytes to {}", content.len(), path.display());
        }
        Commands::Edit {
            path,
            old_string,
            new_string,
            all,
        } => {
            if *all {
                let count = sisyphus_tools::file::edit_all(path, old_string, new_string)?;
                println!("Replaced {} occurrence(s) in {}", count, path.display());
            } else {
                sisyphus_tools::file::edit(path, old_string, new_string)?;
                println!("Replaced first occurrence in {}", path.display());
            }
        }
        Commands::Glob {
            pattern,
            root,
            limit,
        } => {
            let files = sisyphus_tools::file::glob(pattern, root)?;
            let shown = files.iter().take(*limit);
            for f in shown {
                println!("{}", f.display());
            }
            if files.len() > *limit {
                println!("... and {} more", files.len() - *limit);
            } else if files.is_empty() {
                println!("No files matched: {pattern}");
            }
        }
        Commands::Grep {
            pattern,
            path,
            include,
        } => {
            let matches = sisyphus_tools::file::grep(pattern, path, include.as_deref())?;
            for m in &matches {
                println!("{}:{} — {}", m.file.display(), m.line, m.content.trim());
            }
            if matches.is_empty() {
                println!("No matches for pattern: {pattern}");
            }
        }
        Commands::Exec { command, dir } => {
            // Permission check (same as TUI shell)
            match cli_perms.check("exec") {
                Some(true) => {}
                Some(false) => {
                    anyhow::bail!("Permission denied: shell execution is blocked by config");
                }
                None => {
                    eprint!("⚠  Allow shell execution? [Y/n] ");
                    use std::io::Write;
                    std::io::stderr().flush().ok();
                    let mut buf = String::new();
                    std::io::stdin().read_line(&mut buf).ok();
                    let answer = buf.trim().to_lowercase();
                    if !answer.is_empty() && answer != "y" && answer != "yes" {
                        anyhow::bail!("Shell execution denied by user");
                    }
                }
            }
            let dir_ref = dir.as_deref();
            let output = sisyphus_tools::bash::execute(command, dir_ref, 30000).await?;
            if !output.stdout.is_empty() {
                println!("{}", output.stdout);
            }
            if !output.stderr.is_empty() {
                eprintln!("{}", output.stderr);
            }
            println!(
                "Exit: {} ({}ms)",
                output.exit_code,
                output.duration.as_millis()
            );
        }
        Commands::Search { query, num } => {
            let results = sisyphus_tools::web::websearch(query, *num).await?;
            for (i, r) in results.iter().enumerate() {
                println!("[{i}] {} -- {}", r.title, r.url);
                println!("    {}", &r.snippet[..r.snippet.len().min(120)]);
            }
        }

        Commands::ApplyPatch { file, input } => {
            let patch_content = if input.to_string_lossy() == "-" {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf
            } else {
                std::fs::read_to_string(input)?
            };
            let modified = sisyphus_tools::patch::apply_patch(file, &patch_content)?;
            for f in &modified {
                println!("Patched: {f}");
            }
        }

        // ── todo ────────────────────────────────────────
        Commands::Todo(sub) => match sub {
            TodoCommands::Add { content, priority } => {
                let item = sisyphus_tools::todo_add(&todo_db, content, priority.clone().into())?;
                println!("Created: {} ({})", item.id, item.status);
            }
            TodoCommands::List { status } => {
                let filter = status
                    .as_deref()
                    .and_then(|s| s.parse::<sisyphus_tools::TodoStatus>().ok());
                let items = sisyphus_tools::todo_list(&todo_db, filter)?;
                if items.is_empty() {
                    println!("No todos found.");
                } else {
                    let rows: Vec<Vec<String>> = items.iter().map(|item| vec![
                        short_id(&item.id),
                        item.content.clone(),
                        item.status.to_string(),
                        item.priority.to_string(),
                    ]).collect();
                    print_table(&["ID", "Content", "Status", "Priority"], &rows);
                }
            }
            TodoCommands::Update {
                id,
                content,
                status,
                priority,
            } => {
                let s = status
                    .as_deref()
                    .and_then(|s| s.parse::<sisyphus_tools::TodoStatus>().ok());
                let p = priority.clone().map(|p| p.into());
                let item = sisyphus_tools::todo_update(&todo_db, id, content.as_deref(), s, p)?;
                println!("Updated: {} — {}", item.id, item.status);
            }
            TodoCommands::Delete { id, yes: true } => {
                sisyphus_tools::todo_delete(&todo_db, id)?;
                println!("Deleted: {id}");
            }
            TodoCommands::Delete { id, yes: false } => {
                eprint!("Delete todo {id}? [y/N] ");
                std::io::stdout().flush()?;
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                let answer = answer.trim().to_lowercase();
                if answer == "y" || answer == "yes" {
                    sisyphus_tools::todo_delete(&todo_db, id)?;
                    println!("Deleted: {id}");
                } else {
                    println!("Cancelled.");
                }
            }
        },

        // ── session ─────────────────────────────────────
        Commands::Session(sub) => match sub {
            SessionCommands::Create { name } => {
                let session = sisyphus_tools::session_create(&session_db, name)?;
                println!("Created session: {} ({})", session.id, session.name);
            }
            SessionCommands::List => {
                let sessions = sisyphus_tools::session_list(&session_db)?;
                if sessions.is_empty() {
                    println!("No sessions.");
                } else {
                    let rows: Vec<Vec<String>> = sessions.iter().map(|s| vec![
                        short_id(&s.id),
                        s.name.clone(),
                        format!("{} msgs", s.message_count),
                    ]).collect();
                    print_table(&["ID", "Name", "Messages"], &rows);
                }
            }
            SessionCommands::Get { id } => {
                let session = sisyphus_tools::session_get(&session_db, id)?;
                println!("Session: {} ({})", session.id, session.name);
                for msg in &session.messages {
                    println!("  [{}] {}", msg.role, msg.content);
                }
            }
            SessionCommands::Delete { id, yes: true } => {
                sisyphus_tools::session_delete(&session_db, id)?;
                println!("Deleted session: {id}");
            }
            SessionCommands::Delete { id, yes: false } => {
                eprint!("Delete session {id}? [y/N] ");
                std::io::stdout().flush()?;
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                let answer = answer.trim().to_lowercase();
                if answer == "y" || answer == "yes" {
                    sisyphus_tools::session_delete(&session_db, id)?;
                    println!("Deleted session: {id}");
                } else {
                    println!("Cancelled.");
                }
            }
            SessionCommands::AddMessage { id, role, content } => {
                let session = sisyphus_tools::session_add_message(&session_db, id, role, content)?;
                println!(
                    "Message added to {} ({} msgs)",
                    session.id, session.message_count
                );
            }
            SessionCommands::Rename { id, name } => {
                let session = sisyphus_tools::session_rename(&session_db, id, name)?;
                println!("Renamed session {} to {}", session.id, session.name);
            }
            SessionCommands::Fork { id, name } => {
                let session = sisyphus_tools::session_fork(&session_db, id, name)?;
                println!(
                    "Forked session {} from {} — {} messages",
                    session.id, id, session.message_count
                );
            }
            SessionCommands::Continue { id } => {
                let session = sisyphus_tools::session_continue(&session_db, id)?;
                println!(
                    "Continued session {} — {} messages",
                    session.id, session.message_count
                );
            }
            SessionCommands::Undo { id } => {
                let session = sisyphus_tools::session_undo(&session_db, id)?;
                println!(
                    "Undid last message — session {} now has {} messages",
                    session.id, session.message_count
                );
            }
            SessionCommands::Export { id, output, format } => {
                match format.as_str() {
                    "html" => sisyphus_tools::session_export_html(&session_db, id, output)?,
                    "json" => sisyphus_tools::session_export(&session_db, id, output)?,
                    other => anyhow::bail!("unsupported export format '{other}'; use 'json' or 'html'"),
                }
                println!("Exported session {} to {} ({})", id, output.display(), format);
            }
            SessionCommands::Import { input } => {
                let session = sisyphus_tools::session_import(&session_db, input)?;
                println!("Imported session {} ({})", session.id, session.name);
            }
            SessionCommands::Compress { id, threshold } => {
                let strategy = match threshold {
                    Some(t) => sisyphus_tools::CompressStrategy::Threshold(*t),
                    None => sisyphus_tools::CompressStrategy::default(),
                };
                let session = sisyphus_tools::session_compress(&session_db, id, &strategy)?;
                println!(
                    "Compressed session {} — now has {} messages",
                    session.id, session.message_count
                );
            }
        },

        // ── plan ────────────────────────────────────────
        Commands::Plan(sub) => match sub {
            PlanCommands::Create {
                name,
                description,
                file,
            } => {
                let reqs = match file {
                    Some(p) => std::fs::read_to_string(p)?,
                    None => description.clone(),
                };
                let plan = sisyphus_tools::plan_from_requirements(name, &reqs);
                println!("Plan: {} ({} steps)", plan.name, plan.steps.len());
                println!("Status: {}", plan.status);
                for step in &plan.steps {
                    println!("  {} — {}", step.id, step.description);
                }
            }
            PlanCommands::Save { plan_file, output } => {
                let plan = sisyphus_tools::plan_load(plan_file)?;
                sisyphus_tools::plan_save(&plan, output)?;
                println!("Saved plan to {}", output.display());
            }
            PlanCommands::Show { plan_file } => {
                let plan = sisyphus_tools::plan_load(plan_file)?;
                println!("Plan: {} — {} ({})", plan.name, plan.status, plan.id);
                println!("{}", plan.description);
                for step in &plan.steps {
                    println!("  {} — {} [{}]", step.id, step.description, step.status);
                }
            }
            PlanCommands::Review { plan_file } => {
                let mut plan = sisyphus_tools::plan_load(plan_file)?;
                let issues = sisyphus_tools::plan_review(&plan);
                if issues.is_empty() {
                    println!("No issues found.");
                    sisyphus_tools::plan_approve(&mut plan);
                } else {
                    for issue in &issues {
                        println!("  ⚠ {issue}");
                    }
                }
            }
            PlanCommands::Approve { plan_file } => {
                let mut plan = sisyphus_tools::plan_load(plan_file)?;
                sisyphus_tools::plan_approve(&mut plan);
                sisyphus_tools::plan_save(&plan, plan_file)?;
                println!("Approved plan: {}", plan.name);
            }
        },

        // ── oracle ──────────────────────────────────────
        Commands::Oracle(sub) => match sub {
            OracleCommands::AnalyzeFile { path } => {
                let analysis = sisyphus_tools::analyze_file(path)?;
                println!("File: {}", path.display());
                println!("  Lines: {}", analysis.line_count);
                println!("  Imports:");
                for imp in &analysis.imports {
                    println!("    {imp}");
                }
                println!("  Functions:");
                for func in &analysis.functions {
                    println!("    {} at line {}", func.name, func.line);
                }
            }
            OracleCommands::AnalyzeDir { path, include } => {
                let summary = sisyphus_tools::analyze_directory(path, include.as_deref())?;
                println!("Directory: {}", path.display());
                println!("  Files analyzed: {}", summary.file_count);
                println!("  Total lines: {}", summary.total_lines);
            }
            OracleCommands::CodeReview { paths } => {
                let path_refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
                let issues = sisyphus_tools::code_review(&path_refs)?;
                if issues.is_empty() {
                    println!("No issues found.");
                } else {
                    for issue in &issues {
                        println!(
                            "  [{}] {} (line {}) — {}",
                            issue.severity,
                            issue.file.display(),
                            issue.line,
                            issue.message
                        );
                    }
                }
            }
        },

        // ── explore ─────────────────────────────────────
        Commands::Explore(sub) => match sub {
            ExploreCommands::Search {
                patterns,
                paths,
                max_matches: _,
                context_lines,
                include,
            } => {
                let root = paths
                    .first()
                    .map(|p| p.as_path())
                    .unwrap_or_else(|| std::path::Path::new("."));
                let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
                let results = sisyphus_tools::search_patterns(
                    &pattern_refs,
                    root,
                    include.as_deref(),
                    *context_lines,
                )?;
                for m in &results.matches {
                    println!("{}:{} — {}", m.file.display(), m.line, m.content.trim());
                }
                if results.matches.is_empty() {
                    println!("No matches found.");
                }
            }
            ExploreCommands::FindFiles {
                pattern,
                root,
                limit: _,
            } => {
                let files = sisyphus_tools::find_files(pattern, root, None)?;
                for f in &files {
                    println!("{}", f.display());
                }
                if files.is_empty() {
                    println!("No files matched.");
                }
            }
        },

        // ── librarian ───────────────────────────────────
        Commands::Librarian(sub) => match sub {
            LibrarianCommands::Research { query, scope, num } => {
                let scope_enum: sisyphus_tools::ResearchScope =
                    scope.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?;
                let report = sisyphus_tools::research(query, scope_enum, *num).await?;
                println!("=== Research Report ===");
                println!("Query: {}", report.query);
                println!("Sources: {}", report.sources_consulted);
                println!("Confidence: {:.2}", report.confidence);
                println!("{}", report.synthesis);
                println!("\n--- Detailed Findings ---");
                for (i, f) in report.findings.iter().enumerate() {
                    println!("[{i}] {} (relevance: {:.1})", f.claim, f.relevance);
                    println!("    source: {}", f.source_url);
                }
            }
            LibrarianCommands::FetchDocs { url } => {
                let page = sisyphus_tools::fetch_documentation(url).await?;
                println!("Title: {}", page.title);
                println!("Content length: {} bytes", page.content_length);
                println!("--- Preview ---");
                println!("{}", page.content_preview);
            }
            LibrarianCommands::Examples { query, language } => {
                let examples = sisyphus_tools::find_examples(query, language).await?;
                if examples.is_empty() {
                    println!("No examples found for '{}'.", query);
                } else {
                    println!("Found {} example(s):", examples.len());
                    for (i, ex) in examples.iter().enumerate() {
                        println!("[{i}] {} (from {})", ex.description, ex.source);
                        println!("    language: {}", ex.language);
                        if ex.code.len() > 200 {
                            println!("    code ({} chars): {}...", ex.code.len(), &ex.code[..200]);
                        } else {
                            println!("    code: {}", ex.code);
                        }
                    }
                }
            }
            LibrarianCommands::Synthesise { report_file } => {
                let content = std::fs::read_to_string(report_file)?;
                let findings: Vec<sisyphus_tools::ResearchFinding> =
                    serde_json::from_str(&content)?;
                let synthesis = sisyphus_tools::synthesise(&findings, llm_client.as_ref())
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("{}", synthesis);
            }
        },

        // ── momus ────────────────────────────────────────
        Commands::Momus(sub) => match sub {
            MomusCommands::ReviewPlan { plan_file } => {
                let content = std::fs::read_to_string(plan_file)?;
                let name = plan_file
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unnamed".to_string());
                let review =
                    sisyphus_tools::review_plan(llm_client.as_ref(), &content, &name).await;
                println!("{}", review.summary);
                if !review.comments.is_empty() {
                    println!("\n--- Issues ---");
                    for c in &review.comments {
                        println!("  [{}] {} — {}", c.severity, c.category, c.message);
                        if let Some(s) = &c.suggestion {
                            println!("       suggestion: {s}");
                        }
                    }
                }
            }
            MomusCommands::ReviewCode { path } => {
                let content = std::fs::read_to_string(path)?;
                let review = sisyphus_tools::review_code(llm_client.as_ref(), path, &content)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("{}", review.summary);
                if !review.comments.is_empty() {
                    println!("\n--- Issues ---");
                    for c in &review.comments {
                        if let Some(line) = c.line {
                            println!(
                                "  [{}] {}:{} — {} ({})",
                                c.severity,
                                c.file.as_deref().unwrap_or("?"),
                                line,
                                c.message,
                                c.category
                            );
                        } else {
                            println!("  [{}] {} — {}", c.severity, c.message, c.category);
                        }
                        if let Some(s) = &c.suggestion {
                            println!("       suggestion: {s}");
                        }
                    }
                }
            }
            MomusCommands::ReviewArch { text } => {
                let review = sisyphus_tools::review_architecture(llm_client.as_ref(), text).await;
                println!("{}", review.summary);
                if !review.comments.is_empty() {
                    println!("\n--- Issues ---");
                    for c in &review.comments {
                        println!("  [{}] {} — {}", c.severity, c.category, c.message);
                        if let Some(s) = &c.suggestion {
                            println!("       suggestion: {s}");
                        }
                    }
                }
            }
            MomusCommands::ReviewDir { path } => {
                let walker = std::fs::read_dir(path)?;
                let mut reviews = Vec::new();
                for entry in walker {
                    let entry = entry?;
                    let file_path = entry.path();
                    if file_path.extension().map(|e| e == "rs").unwrap_or(false) {
                        if let Ok(content) = std::fs::read_to_string(&file_path) {
                            if let Ok(review) = sisyphus_tools::review_code(
                                llm_client.as_ref(),
                                &file_path,
                                &content,
                            )
                            .await
                            {
                                reviews.push(review);
                            }
                        }
                    }
                }
                let total_passed = reviews.iter().filter(|r| r.passed).count();
                let total_failed = reviews.iter().filter(|r| !r.passed).count();
                println!(
                    "Reviewed {} files: {} passed, {} failed",
                    reviews.len(),
                    total_passed,
                    total_failed
                );
                for r in &reviews {
                    println!("  {} — {}", r.target, if r.passed { "✅" } else { "❌" });
                    for c in &r.comments {
                        if let Some(line) = c.line {
                            println!("    [{}] line {}: {}", c.severity, line, c.message);
                        } else {
                            println!("    [{}] {}", c.severity, c.message);
                        }
                    }
                }
            }
        },

        // ── metis ────────────────────────────────────────
        Commands::Metis(sub) => match sub {
            MetisCommands::Analyse { request, file } => {
                let text = match file {
                    Some(f) => std::fs::read_to_string(f)?,
                    None => request.join(" "),
                };
                if text.trim().is_empty() {
                    anyhow::bail!("No request text provided. Pass inline text or use --file.");
                }
                let analysis = sisyphus_tools::metis::analyse(&text, llm_client.as_ref())
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("=== Metis Analysis ===");
                println!("Summary: {}", analysis.summary);
                if !analysis.ambiguities.is_empty() {
                    println!("\n--- Ambiguities ---");
                    for a in &analysis.ambiguities {
                        println!("  [{}] {} — {}", a.severity, a.aspect, a.description);
                        if !a.options.is_empty() {
                            println!("    options: {}", a.options.join(" | "));
                        }
                        if let Some(r) = &a.recommended {
                            println!("    recommended: {r}");
                        }
                    }
                }
                if !analysis.implicit_requirements.is_empty() {
                    println!("\n--- Implicit Requirements ---");
                    for i in &analysis.implicit_requirements {
                        println!(
                            "  [conf: {:.1}] {} — {}",
                            i.confidence, i.description, i.rationale
                        );
                    }
                }
            }
        },

        // ── skill ───────────────────────────────────────
        Commands::Skill(sub) => match sub {
            SkillCommands::List => {
                skill::run_list()?;
            }
            SkillCommands::Show { name } => {
                skill::run_show(name)?;
            }
            SkillCommands::Install { path } => {
                skill::run_install(path)?;
            }
            SkillCommands::Remove { name } => {
                skill::run_remove(name)?;
            }
        },

        // ── rules ────────────────────────────────────────
        Commands::Rules(sub) => match sub {
            RulesCommands::Show => {
                let root = std::env::current_dir()?;
                let rules = sisyphus_tools::load_project_rules(&root);
                match &rules.source {
                    Some(src) => println!("Project rules loaded from: {src}"),
                    None => println!("No AGENTS.md found (checked: AGENTS.md, .opencode/AGENTS.md, docs/AGENTS.md)"),
                }
                if rules.agents.is_empty() {
                    println!("No project-defined agents.");
                } else {
                    println!("Project-defined agents ({}):", rules.agents.len());
                    for a in &rules.agents {
                        println!("  - {} ({})", a.agent_id, a.display_name);
                        println!("    description: {}", a.description);
                        if !a.tool_prefixes.is_empty() {
                            println!("    tools: {}", a.tool_prefixes.join(", "));
                        }
                    }
                }
            }
            RulesCommands::Reload => {
                let root = std::env::current_dir()?;
                let rules = sisyphus_tools::load_project_rules(&root);
                match &rules.source {
                    Some(src) => {
                        let count = rules.agents.len();
                        println!("Reloaded {count} project agent(s) from {src}");
                    }
                    None => println!("No AGENTS.md found."),
                }
            }
        },

        // ── lsp ─────────────────────────────────────────
        Commands::Lsp(sub) => match sub {
            LspCommands::Diagnostics {
                file,
                language_id,
                server,
                server_args,
                root_uri,
            } => {
                lsp::run_diagnostics(file, language_id, server, server_args, root_uri).await?;
            }
            LspCommands::GotoDef {
                file,
                line,
                col,
                language_id,
                server,
                server_args,
                root_uri,
            } => {
                lsp::run_goto_def(
                    file,
                    *line,
                    *col,
                    language_id,
                    server,
                    server_args,
                    root_uri,
                )
                .await?;
            }
            LspCommands::FindRefs {
                file,
                line,
                col,
                language_id,
                server,
                server_args,
                root_uri,
            } => {
                lsp::run_find_refs(
                    file,
                    *line,
                    *col,
                    language_id,
                    server,
                    server_args,
                    root_uri,
                )
                .await?;
            }
            LspCommands::Hover {
                file,
                line,
                col,
                language_id,
                server,
                server_args,
                root_uri,
            } => {
                lsp::run_hover(
                    file,
                    *line,
                    *col,
                    language_id,
                    server,
                    server_args,
                    root_uri,
                )
                .await?;
            }
            LspCommands::Symbols {
                file,
                language_id,
                server,
                server_args,
                root_uri,
            } => {
                lsp::run_symbols(file, language_id, server, server_args, root_uri).await?;
            }
        },

        // ── delegate ────────────────────────────────────
        Commands::Delegate(sub) => match sub {
            DelegateCommands::Run {
                file,
                max_concurrent,
            } => {
                delegate::run_tasks(file.clone(), *max_concurrent).await?;
            }
            DelegateCommands::Inline {
                commands,
                max_concurrent,
            } => {
                delegate::run_inline(commands, *max_concurrent).await?;
            }
        },

        // ── mcp ─────────────────────────────────────────
        Commands::Mcp(sub) => match sub {
            McpCommands::Info { connect } => {
                mcp::run_info(connect).await?;
            }
            McpCommands::Call {
                tool,
                args,
                connect,
            } => {
                mcp::run_call(tool, args.as_deref(), connect).await?;
            }
            McpCommands::Serve => {
                mcp::run_serve(llm_client.clone()).await?;
            }
        },

        // ── engine ──────────────────────────────────────
        Commands::Sop(sub) => match sub {
            SopCommands::Run { sop_id } => {
                let mut client = bridge::BridgeClient::connect(&bridge_addr).await?;
                let result = client.execute_sop(sop_id).await?;
                println!("{result}");
            }
            SopCommands::List => {
                let mut client = bridge::BridgeClient::connect(&bridge_addr).await?;
                let sops = client.list_sops().await?;
                if sops.is_empty() {
                    println!("No SOPs registered.");
                } else {
                    for sop in &sops {
                        println!("  {} ({} nodes)", sop.sop_id, sop.node_count);
                    }
                }
            }
        },
        // ── git ─────────────────────────────────────────
        Commands::Git(sub) => match sub {
            GitCommands::Status { cwd } => git::run_status(cwd.clone()).await?,
            GitCommands::Diff {
                pathspec,
                cached,
                cwd,
            } => git::run_diff(pathspec.clone(), *cached, cwd.clone()).await?,
            GitCommands::Add { pathspecs, cwd } => {
                git::run_add(pathspecs.clone(), cwd.clone()).await?
            }
            GitCommands::Commit {
                message,
                allow_empty,
                cwd,
            } => git::run_commit(message.clone(), *allow_empty, cwd.clone()).await?,
            GitCommands::Log { count, cwd } => git::run_log(*count, cwd.clone()).await?,
            GitCommands::Branch { cwd } => git::run_branch(cwd.clone()).await?,
            GitCommands::Checkout {
                branch,
                create,
                cwd,
            } => git::run_checkout(branch.clone(), *create, cwd.clone()).await?,
            GitCommands::Push {
                remote,
                branch,
                force,
                cwd,
            } => git::run_push(remote.clone(), branch.clone(), *force, cwd.clone()).await?,
            GitCommands::Pull {
                remote,
                branch,
                cwd,
            } => git::run_pull(remote.clone(), branch.clone(), cwd.clone()).await?,
            GitCommands::Summary { cwd } => git::run_summary(cwd.clone()).await?,
        },

        // ── build ────────────────────────────────────────
        Commands::Build(sub) => match sub {
            BuildCommands::Check {
                package,
                all,
                lang,
                cwd,
            } => build::run_check(package.clone(), *all, cwd.clone(), lang.clone()).await?,
            BuildCommands::Build {
                package,
                release,
                lang,
                cwd,
            } => build::run_build(package.clone(), *release, cwd.clone(), lang.clone()).await?,
            BuildCommands::Test {
                package,
                name,
                all,
                lang,
                cwd,
            } => {
                build::run_test(
                    package.clone(),
                    name.clone(),
                    *all,
                    cwd.clone(),
                    lang.clone(),
                )
                .await?
            }
            BuildCommands::Lint { package, lang, cwd } => {
                build::run_lint(package.clone(), cwd.clone(), lang.clone()).await?
            }
            BuildCommands::Fmt { check, lang, cwd } => {
                build::run_fmt(*check, cwd.clone(), lang.clone()).await?
            }
            BuildCommands::Doc { package, lang, cwd } => {
                build::run_doc(package.clone(), cwd.clone(), lang.clone()).await?
            }
        },

        // ── run ────────────────────────────────────────────
        Commands::Run(sub) => match sub {
            RunCommands::Run { args, lang, cwd } => {
                run::run_run(args.clone(), lang.clone(), cwd.clone()).await?
            }
        },

        // ── init ───────────────────────────────────────────
        Commands::Init(sub) => match sub {
            InitCommands::List => init::run_list()?,
            InitCommands::Generate {
                template,
                name,
                dir,
            } => init::run_generate(template, name, dir)?,
        },

        // ── precommit ──────────────────────────────────────
        Commands::Precommit(sub) => match sub {
            PrecommitCommands::Install {
                dir,
                no_fmt,
                no_lint,
                allow_warnings,
                lang,
            } => precommit::run_install(
                dir.clone(),
                *no_fmt,
                *no_lint,
                *allow_warnings,
                lang.clone(),
            )?,
            PrecommitCommands::Uninstall { dir } => precommit::run_uninstall(dir.clone())?,
            PrecommitCommands::Status { dir } => precommit::run_status(dir.clone())?,
        },
        // ── pr ──────────────────────────────────────────────────
        Commands::Pr(sub) => match sub {
            PrCommands::List => {
                let prs = sisyphus_tools::PrManager::list_prs()?;
                if prs.is_empty() {
                    println!("No open pull requests found.");
                } else {
                    println!("{:<6} {:<40} {:<15} {:<10}", "Num", "Title", "Author", "State");
                    println!("{}", "-".repeat(71));
                    for pr in prs {
                        println!(
                            "{:<6} {:<40} {:<15} {:<10}",
                            pr.number,
                            pr.title.chars().take(37).collect::<String>() + if pr.title.len() > 37 { "..." } else { "" },
                            pr.author,
                            pr.state
                        );
                    }
                }
            }
            PrCommands::Checkout { number } => {
                sisyphus_tools::PrManager::checkout_pr(*number)?;
                println!("Checked out PR #{}", number);
            }
            PrCommands::Review { number, post, format } => {
                // Detect CI mode from environment
                let ci_cfg = sisyphus_tools::CiConfig::from_env();
                let use_json = format == "json" || ci_cfg.output_format == "json";

                if use_json {
                    println!("{{\"status\":\"running\",\"pr_number\":{},\"message\":\"Running LLM review...\"}}", number);
                } else {
                    println!("Running LLM review for PR #{}...", number);
                }

                let review = sisyphus_tools::PrManager::review_pr(*number, llm_client.clone()).await?;

                if use_json {
                    // JSON output for CI consumption
                    let escaped_review = review
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\n', "\\n")
                        .replace('\r', "\\r");
                    println!("{{\"status\":\"complete\",\"pr_number\":{},\"review\":\"{}\"}}", number, escaped_review);
                } else {
                    println!("\n=== PR Review Result ===\n");
                    println!("{}", review);
                }

                // Post as PR comment if requested or in CI mode
                if *post || ci_cfg.ci_mode {
                    match sisyphus_tools::post_pr_comment(*number, &review) {
                        Ok(url) => {
                            if use_json {
                                println!("{{\"status\":\"comment_posted\",\"pr_number\":{},\"url\":\"{}\"}}", number, url);
                            } else {
                                println!("\nReview comment posted: {}", url);
                            }
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to post PR comment: {}", e);
                            if ci_cfg.ci_mode {
                                // In CI mode, failing to post is non-fatal
                                eprintln!("(non-fatal in CI mode)");
                            }
                        }
                    }
                }
            }
        },
        // ── wisdom ──────────────────────────────────────────────
        Commands::Wisdom(sub) => match sub {
            WisdomCommands::Record { category, content } => {
                sisyphus_tools::wisdom_record(category, content).await?;
                println!("Recorded wisdom in category: {category}");
            }
            WisdomCommands::List => {
                let categories = sisyphus_tools::wisdom_list().await?;
                if categories.is_empty() {
                    println!("No wisdom categories found.");
                } else {
                    println!("Available wisdom categories:");
                    for cat in &categories {
                        println!("  {cat}");
                    }
                }
            }
            WisdomCommands::Show { category } => {
                let content = sisyphus_tools::wisdom_get(category).await?;
                println!("--- Wisdom: {} ---\n{content}", category);
            }
            WisdomCommands::Delete { category } => {
                sisyphus_tools::wisdom_delete(category).await?;
                println!("Deleted wisdom category: {category}");
            }
            WisdomCommands::Inject => {
                let context = sisyphus_tools::wisdom_inject().await?;
                if context.is_empty() {
                    println!("No wisdom to inject.");
                } else {
                    println!("{context}");
                }
            }
        },
        // ── hashline ────────────────────────────────────────
        Commands::Hashline(sub) => match sub {
            HashlineCommands::Annotate { path } => {
                let count = sisyphus_tools::hashline::annotate_file(path)?;
                println!("Annotated {} lines in {}", count, path.display());
            }
            HashlineCommands::Verify { path } => {
                let content = std::fs::read_to_string(path)?;
                let mut valid = 0;
                let mut invalid = 0;
                
                for (i, line) in content.lines().enumerate() {
                    if sisyphus_tools::hashline::parse_hashline(line).is_some() {
                        if sisyphus_tools::hashline::verify_line(&content, line).unwrap_or(false) {
                            valid += 1;
                        } else {
                            invalid += 1;
                            eprintln!("Invalid hashline at line {}: {}", i + 1, line.trim());
                        }
                    }
                }
                println!("Verification complete: {} valid, {} invalid", valid, invalid);
                if invalid > 0 {
                    std::process::exit(1);
                }
            }
            HashlineCommands::Edit {
                path,
                hashline,
                new_content,
            } => {
                let old = sisyphus_tools::hashline::edit_with_hashline(path, hashline, new_content)?;
                println!("Successfully edited line. Old content: '{}'", old);
            }
        },
        // ── diagnose ───────────────────────────────────────
        Commands::Diagnose(sub) => match sub {

            DiagnoseCommands::Run {
                lang,
                package,
                all,
                no_lint,
                cwd,
            } => {
                diagnose::run_diagnose(lang.clone(), package.clone(), *all, *no_lint, cwd.clone())?
            }
        },
        // ── fix ─────────────────────────────────────────────
        Commands::Fix(sub) => match sub {
            FixCommands::Run {
                lang,
                package,
                lint_fixes,
                all,
                cwd,
            } => diagnose::run_fix(
                lang.clone(),
                package.clone(),
                *lint_fixes,
                *all,
                cwd.clone(),
            )?,
        },

        // ── github ──────────────────────────────────────────
        Commands::Github(sub) => match sub {
            GithubCommands::Action {
                kind,
                output,
                toolchain,
                cargo_flags: _,
                exclude,
                clippy,
                deny,
            } => {
                let wk = match kind.as_str() {
                    "ci" => sisyphus_tools::WorkflowKind::Ci,
                    "pr-review" => sisyphus_tools::WorkflowKind::PrReview,
                    "release" => sisyphus_tools::WorkflowKind::Release,
                    other => anyhow::bail!(
                        "Unknown workflow kind: {other}. Use: ci, pr-review, release"
                    ),
                };
                let opts = sisyphus_tools::GenerateOptions {
                    rust_toolchain: toolchain.clone(),
                    cargo_extra_flags: String::new(),
                    exclude_packages: exclude.clone(),
                    include_clippy: *clippy,
                    include_deny: *deny,
                };
                let path = match output {
                    Some(p) => {
                        let content = sisyphus_tools::generate_workflow(wk, &opts);
                        std::fs::write(p, content)?;
                        println!("Workflow written to {}", p.display());
                        p.clone()
                    }
                    None => sisyphus_tools::write_workflow(wk, &opts)?,
                };
                println!("Generated {} workflow at {}", wk.as_str(), path.display());
            }
            GithubCommands::CiConfig => {
                let cfg = sisyphus_tools::CiConfig::from_env();
                println!("CI mode: {}", cfg.ci_mode);
                println!("Output format: {}", cfg.output_format);
                match &cfg.github_token {
                    Some(_) => println!("GitHub token: present"),
                    None => println!("GitHub token: not set"),
                }
                match &cfg.pr_number {
                    Some(n) => println!("PR number: {n}"),
                    None => println!("PR number: not set"),
                }
            }
        },
        // ── provider ─────────────────────────────────────────
        Commands::Provider(sub) => {
            let config = &sisyphus_cfg;
            match sub {
                ProviderCommands::List => {
                    let providers = &config.llm.providers;
                    if providers.is_empty() {
                        println!("No providers configured.");
                        println!("  Using model '{}' at {}",
                            config.llm.model,
                            config.llm.endpoint,
                        );
                    } else {
                        println!("Registered providers (fallback order):");
                        for (i, p) in providers.iter().enumerate() {
                            let key_status = if p.api_key.is_empty() || p.api_key.starts_with("env:") {
                                "key: <env>"
                            } else {
                                "key: set"
                            };
                            println!("  {}. {} (kind: {}, model: {}, {})",
                                i + 1, p.name, p.kind, p.model, key_status,
                            );
                        }
                    }
                }
                ProviderCommands::Test { name, prompt } => {
                    let providers = &config.llm.providers;
                    let provider = providers.iter().find(|p| p.name == *name);
                    match provider {
                        Some(p) => {
                            println!("Testing provider '{}' (kind: {}, model: {})...", p.name, p.kind, p.model);
                            let api_key = p.resolve_api_key();
                            if api_key.is_empty() {
                                eprintln!("API key for '{}' is empty. Check env var or config.", p.name);
                            } else {
                                println!("API key resolved ({} chars), endpoint: {}",
                                    api_key.len(),
                                    p.base_url.as_deref().unwrap_or("(default)"),
                                );
                                println!("Prompt: {}", prompt);
                                println!("Test call would go here (API call disabled in CLI test mode).");
                            }
                        }
                        None => {
                            eprintln!("Provider '{}' not found. Use `provider list` to see available providers.", name);
                        }
                    }
                }
                ProviderCommands::Fallback => {
                    let providers = &config.llm.providers;
                    println!("Fallback chain for provider routing:");
                    println!("  1. Try registered providers in order ({} total)", providers.len());
                    for (i, p) in providers.iter().enumerate() {
                        println!("     {}. {} (kind: {}, model: {})",
                            i + 1, p.name, p.kind, p.model);
                    }
                    println!("  2. Fallback to default endpoint: {} with model '{}'",
                        config.llm.endpoint, config.llm.model);
                    println!();
                    println!("Error handling in fallback:");
                    println!("  - Transient/RateLimited errors -> skip to next provider");
                    println!("  - Permanent/Auth errors -> abort immediately");
                    println!("  - All providers exhausted -> fallback to default endpoint");
                }
            }
        }
        // ── tool ──────────────────────────────────────────────
        Commands::Tool(sub) => {
            match sub {
                ToolCommands::RegisterMcp { prefix, command } => {
                    let command_str = command.join(" ");
                    let config_path = cli.config.as_deref()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| {
                            ["opencode.json", "opencode.jsonc", ".opencode.json", "sisyphus.toml"]
                                .iter()
                                .find(|p| std::path::Path::new(p).exists())
                                .map(std::path::PathBuf::from)
                                .unwrap_or_else(|| std::path::PathBuf::from("sisyphus.toml"))
                        });

                    let mut cfg = if config_path.exists() {
                        let path_str = config_path.to_string_lossy();
                        match sisyphus_types::Config::from_file(&path_str) {
                            Ok(c) => c,
                            Err(e) => {
                                eprintln!("Warning: failed to load config: {e}, using defaults");
                                sisyphus_types::Config::default()
                            }
                        }
                    } else {
                        sisyphus_types::Config::default()
                    };

                    cfg.mcp.stdio_servers.insert(prefix.clone(), command_str.clone());

                    let path_str = config_path.to_string_lossy().to_string();
                    cfg.save_to_file(&path_str)?;

                    println!("Registered MCP tool '{}' -> {}", prefix, command_str);
                    println!("Config saved to: {}", config_path.display());
                    println!("Restart the bridge for changes to take effect.");
                }
                ToolCommands::List => {
                    let providers = &sisyphus_cfg.mcp.stdio_servers;
                    let http_servers = &sisyphus_cfg.mcp.http_servers;
                    println!("Registered MCP tools:");
                    if providers.is_empty() && http_servers.is_empty() {
                        println!("  (none)");
                        println!("Use `tool register-mcp <prefix> <command>` to add one.");
                    }
                    if !providers.is_empty() {
                        println!("\n  Stdio servers:");
                        for (prefix, cmd) in providers {
                            println!("    {prefix}: {cmd}");
                        }
                    }
                    if !http_servers.is_empty() {
                        println!("\n  HTTP servers:");
                        for (prefix, url) in http_servers {
                            println!("    {prefix}: {url}");
                        }
                    }
                }
            }
        }
        // ── plugin ─────────────────────────────────────────────
        Commands::Plugin(sub) => {
            let mut client = get_bridge_client(&cli, &sisyphus_cfg).await?;
            match sub {
                PluginCommands::List => {
                    plugin::run_list(&mut client).await?;
                }
                PluginCommands::Load { path } => {
                    plugin::run_load(&mut client, path).await?;
                }
                PluginCommands::Watch { path } => {
                    plugin::run_watch(path.as_deref()).await?;
                }
                PluginCommands::Unload { name } => {
                    plugin::run_unload(&mut client, name).await?;
                }
            }
        }
        Commands::LearningInsights { strategy } => {
            let mut client = get_bridge_client(&cli, &sisyphus_cfg).await?;
            remote::run_learning_insights(&mut client, strategy.as_deref()).await?;
        }
        Commands::Health => {
            let mut client = bridge::BridgeClient::connect(&bridge_addr).await?;
            let resp = client.health_check().await?;
            println!("Status: {}", resp.status);
            println!("Version: {}", resp.version);
        }
        Commands::Status => {
            let mut client = bridge::BridgeClient::connect(&bridge_addr).await?;
            remote::run_status(&mut client).await?;
        }
        Commands::Remote(sub) => {
            let mut client = bridge::BridgeClient::connect(&bridge_addr).await?;
            match sub {
                RemoteCommands::Bench => remote::run_benchmarks(&mut client).await?,
                RemoteCommands::Agents => remote::run_list_agents(&mut client).await?,
                RemoteCommands::Tools => remote::run_list_tools(&mut client).await?,
                RemoteCommands::Skills => remote::run_list_skills(&mut client).await?,
                RemoteCommands::Exec { sop_id } => {
                    remote::run_exec_sop(&mut client, sop_id).await?
                }
                RemoteCommands::Decompose { goal } => {
                    remote::run_decompose_goal(&mut client, goal).await?
                }
            }
        }
        Commands::Daemon => {
            println!("Starting Sisyphus bridge daemon...");
            let target_dir = std::path::Path::new("target");
            let binary = if cfg!(debug_assertions) {
                target_dir.join("debug").join("sisyphus-bridge")
            } else {
                target_dir.join("release").join("sisyphus-bridge")
            };
            let child = if binary.exists() {
                std::process::Command::new(&binary)
                    .spawn()
                    .with_context(|| format!("Failed to start {}", binary.display()))?
            } else {
                println!(
                    "Binary not found at {}, falling back to cargo run...",
                    binary.display()
                );
                std::process::Command::new("cargo")
                    .args(["run", "-p", "sisyphus-bridge", "--release"])
                    .spawn()
                    .context("Failed to start bridge daemon via cargo run")?
            };
            println!(
                "Bridge daemon started (PID: {}). Running in background.",
                child.id()
            );
            println!("Use Ctrl+C to stop the daemon.");
            std::thread::spawn(move || {
                let _ = child.wait_with_output();
            });
        }
    }

    Ok(())
}

async fn get_bridge_client(cli: &Cli, config: &sisyphus_types::Config) -> Result<bridge::BridgeClient> {
    let addr = cli
        .bridge
        .clone()
        .unwrap_or_else(|| config.bridge_address.clone());
    bridge::BridgeClient::connect(&addr).await
}
