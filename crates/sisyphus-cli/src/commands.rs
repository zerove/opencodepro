use std::path::PathBuf;
use clap::{Subcommand, ValueEnum};

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

#[derive(Subcommand)]
pub enum GithubCommands {
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
pub enum ProviderCommands {
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
pub enum ToolCommands {
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
pub enum HashlineCommands {
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
pub enum PrCommands {
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
pub enum WisdomCommands {
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
pub enum RemoteCommands {
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
pub enum PluginCommands {
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
pub enum DiagnoseCommands {
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
pub enum FixCommands {
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
pub enum InitCommands {
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
pub enum PrecommitCommands {
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
pub enum GitCommands {
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
pub enum BuildCommands {
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
pub enum RunCommands {
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
pub enum TodoCommands {
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
pub enum SessionCommands {
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
pub enum PlanCommands {
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
pub enum OracleCommands {
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
pub enum ExploreCommands {
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
pub enum LibrarianCommands {
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
#[allow(clippy::enum_variant_names, reason = "all Momus commands start with Review for naming consistency")]
pub enum MomusCommands {
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
pub enum MetisCommands {
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
pub enum SkillCommands {
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
pub enum RulesCommands {
    /// Show loaded project rules (from AGENTS.md)
    Show,
    /// Reload project rules from AGENTS.md
    Reload,
}

#[derive(Subcommand)]
pub enum LspCommands {
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
pub enum McpCommands {
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
pub enum DelegateCommands {
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
pub enum SopCommands {
    /// Execute an SOP by ID
    Run { sop_id: String },
    /// List available SOPs
    List,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum PriorityArg {
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

pub fn short_id(id: &str) -> String {
    if id.len() <= 12 { id.to_string() } else { format!("{}…", &id[..12]) }
}

pub fn print_table(header: &[&str], rows: &[Vec<String>]) {
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
