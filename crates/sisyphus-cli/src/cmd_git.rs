use std::path::PathBuf;

use clap::Subcommand;

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
