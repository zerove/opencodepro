use std::path::PathBuf;

use clap::Subcommand;

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
