use std::path::PathBuf;

use clap::Subcommand;

use crate::PriorityArg;

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
