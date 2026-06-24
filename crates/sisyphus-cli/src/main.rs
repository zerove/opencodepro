use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

mod bridge;
mod delegate;
mod lsp;
mod mcp;
mod permission;
mod shell;
mod skill;
mod tui;

#[derive(Parser)]
#[command(name = "sisyphus", version, about = "Sisyphus Cognitive Engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable interactive TUI mode
    #[arg(short = 'i', long = "interactive")]
    interactive: bool,

    /// Bridge address (default: http://[::1]:50051)
    #[arg(short = 'b', long = "bridge", env = "SISYPHUS_BRIDGE")]
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

    // ── task tracking ───────────────────────────────────
    /// Manage todos
    #[command(subcommand)]
    Todo(TodoCommands),

    /// Manage sessions
    #[command(subcommand)]
    Session(SessionCommands),

    /// Manage plans
    #[command(subcommand)]
    Plan(PlanCommands),

    /// Code analysis tools
    #[command(subcommand)]
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

    /// Language server protocol tools
    #[command(subcommand)]
    Lsp(LspCommands),

    /// Task orchestration (parallel execution)
    #[command(subcommand)]
    Delegate(DelegateCommands),

    // ── MCP ─────────────────────────────────────────────
    /// MCP server integration (connect, info, call)
    #[command(subcommand)]
    Mcp(McpCommands),

    // ── engine ──────────────────────────────────────────
    /// Interact with the Sisyphus cognitive engine via gRPC bridge
    #[command(subcommand)]
    Sop(SopCommands),
    /// Check bridge health
    Health,
    /// Start the bridge daemon
    Daemon,
}

// ── Sub-command enums ────────────────────────────────────

#[derive(Subcommand)]
enum TodoCommands {
    /// Add a new todo item
    Add { content: String, #[arg(short = 'p', default_value = "medium")] priority: PriorityArg },
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
    Delete { id: String },
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
    Delete { id: String },
    /// Add a message to a session
    AddMessage { id: String, role: String, content: String },
    /// Rename a session
    Rename { id: String, name: String },
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
    AnalyzeDir { path: PathBuf, #[arg(short = 't')] include: Option<String> },
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
        /// Otherwise treated as <command> [args...] → stdio transport.
        #[arg(required = true)]
        connect: Vec<String>,
    },
    /// Call a tool on an MCP server
    Call {
        /// Name of the tool to call
        tool: String,
        /// JSON arguments (optional, e.g. '{"text":"hello"}')
        args: Option<String>,
        /// Server connection: <url> or <command> [args...]
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

// ── Main ─────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // Resolve config: try file, then env, then defaults
    // Config overrides the defaults for data_dir and bridge address
    let (data_dir, bridge_addr) = if let Some(ref config_path) = cli.config {
        match sisyphus_types::Config::from_file(
            &config_path.to_string_lossy()
        ) {
            Ok(cfg) => {
                let data_dir = cli.data_dir.clone().unwrap_or_else(|| {
                    // Derive data_dir from cache.sqlite_path parent, or fallback
                    let sqlite = PathBuf::from(&cfg.cache.sqlite_path);
                    sqlite.parent().unwrap_or(Path::new(".sisyphus")).to_path_buf()
                });
                let bridge = cli.bridge.clone().unwrap_or_else(|| {
                    "http://[::1]:50051".to_string()
                });
                (data_dir, bridge)
            }
            Err(e) => {
                eprintln!("Warning: failed to load config '{}': {e}", config_path.display());
                (cli.data_dir.unwrap_or_else(|| PathBuf::from(".sisyphus")),
                 cli.bridge.unwrap_or_else(|| "http://[::1]:50051".to_string()))
            }
        }
    } else {
        (cli.data_dir.unwrap_or_else(|| PathBuf::from(".sisyphus")),
         cli.bridge.unwrap_or_else(|| "http://[::1]:50051".to_string()))
    };

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

    match cli.command.unwrap() {
        // ── file tools ──────────────────────────────────
        Commands::Read { path } => {
            let content = sisyphus_tools::file::read(&path)?;
            println!("{content}");
        }
        Commands::ReadLines { path, offset, limit } => {
            let content = sisyphus_tools::file::read_lines(&path, offset, limit)?;
            print!("{content}");
        }
        Commands::Write { path, content } => {
            sisyphus_tools::file::write(&path, &content)?;
            println!("Written {} bytes to {}", content.len(), path.display());
        }
        Commands::Edit { path, old_string, new_string, all } => {
            if all {
                let count = sisyphus_tools::file::edit_all(&path, &old_string, &new_string)?;
                println!("Replaced {} occurrence(s) in {}", count, path.display());
            } else {
                sisyphus_tools::file::edit(&path, &old_string, &new_string)?;
                println!("Replaced first occurrence in {}", path.display());
            }
        }
        Commands::Glob { pattern, root, limit } => {
            let files = sisyphus_tools::file::glob(&pattern, &root)?;
            let shown = files.iter().take(limit);
            for f in shown {
                println!("{}", f.display());
            }
            if files.len() > limit {
                println!("... and {} more", files.len() - limit);
            } else if files.is_empty() {
                println!("No files matched: {pattern}");
            }
        }
        Commands::Grep { pattern, path, include } => {
            let matches = sisyphus_tools::file::grep(&pattern, &path, include.as_deref())?;
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
            let output = sisyphus_tools::bash::execute(&command, dir_ref, 30000).await?;
            if !output.stdout.is_empty() {
                println!("{}", output.stdout);
            }
            if !output.stderr.is_empty() {
                eprintln!("{}", output.stderr);
            }
            println!("Exit: {} ({}ms)", output.exit_code, output.duration.as_millis());
        }
        Commands::Search { query, num } => {
            let results = sisyphus_tools::web::websearch(&query, num).await?;
            for (i, r) in results.iter().enumerate() {
                println!("[{i}] {} -- {}", r.title, r.url);
                println!("    {}", &r.snippet[..r.snippet.len().min(120)]);
            }
        }

        // ── todo ────────────────────────────────────────
        Commands::Todo(sub) => match sub {
            TodoCommands::Add { content, priority } => {
                let item = sisyphus_tools::todo_add(&todo_db, &content, priority.into())?;
                println!("Created: {} ({})", item.id, item.status);
            }
            TodoCommands::List { status } => {
                let filter = status.as_deref().and_then(|s| s.parse::<sisyphus_tools::TodoStatus>().ok());
                let items = sisyphus_tools::todo_list(&todo_db, filter)?;
                if items.is_empty() {
                    println!("No todos found.");
                } else {
                    for item in &items {
                        println!("  [{}] {} — {} ({})", item.id, item.content, item.status, item.priority);
                    }
                }
            }
            TodoCommands::Update { id, content, status, priority } => {
                let s = status.as_deref().and_then(|s| s.parse::<sisyphus_tools::TodoStatus>().ok());
                let p = priority.map(|p| p.into());
                let item = sisyphus_tools::todo_update(&todo_db, &id, content.as_deref(), s, p)?;
                println!("Updated: {} — {}", item.id, item.status);
            }
            TodoCommands::Delete { id } => {
                sisyphus_tools::todo_delete(&todo_db, &id)?;
                println!("Deleted: {id}");
            }
        }

        // ── session ─────────────────────────────────────
        Commands::Session(sub) => match sub {
            SessionCommands::Create { name } => {
                let session = sisyphus_tools::session_create(&session_db, &name)?;
                println!("Created session: {} ({})", session.id, session.name);
            }
            SessionCommands::List => {
                let sessions = sisyphus_tools::session_list(&session_db)?;
                if sessions.is_empty() {
                    println!("No sessions.");
                } else {
                    for s in &sessions {
                        println!("  {} — {} ({} msgs)", s.id, s.name, s.message_count);
                    }
                }
            }
            SessionCommands::Get { id } => {
                let session = sisyphus_tools::session_get(&session_db, &id)?;
                println!("Session: {} ({})", session.id, session.name);
                for msg in &session.messages {
                    println!("  [{}] {}", msg.role, msg.content);
                }
            }
            SessionCommands::Delete { id } => {
                sisyphus_tools::session_delete(&session_db, &id)?;
                println!("Deleted session: {id}");
            }
            SessionCommands::AddMessage { id, role, content } => {
                let session = sisyphus_tools::session_add_message(&session_db, &id, &role, &content)?;
                println!("Message added to {} ({} msgs)", session.id, session.message_count);
            }
            SessionCommands::Rename { id, name } => {
                let session = sisyphus_tools::session_rename(&session_db, &id, &name)?;
                println!("Renamed session {} to {}", session.id, session.name);
            }
        }

        // ── plan ────────────────────────────────────────
        Commands::Plan(sub) => match sub {
            PlanCommands::Create { name, description, file } => {
                let reqs = match file {
                    Some(p) => std::fs::read_to_string(&p)?,
                    None => description,
                };
                let plan = sisyphus_tools::plan_from_requirements(&name, &reqs);
                println!("Plan: {} ({} steps)", plan.name, plan.steps.len());
                println!("Status: {}", plan.status);
                for step in &plan.steps {
                    println!("  {} — {}", step.id, step.description);
                }
            }
            PlanCommands::Save { plan_file, output } => {
                let plan = sisyphus_tools::plan_load(&plan_file)?;
                sisyphus_tools::plan_save(&plan, &output)?;
                println!("Saved plan to {}", output.display());
            }
            PlanCommands::Show { plan_file } => {
                let plan = sisyphus_tools::plan_load(&plan_file)?;
                println!("Plan: {} — {} ({})", plan.name, plan.status, plan.id);
                println!("{}", plan.description);
                for step in &plan.steps {
                    println!("  {} — {} [{}]", step.id, step.description, step.status);
                }
            }
            PlanCommands::Review { plan_file } => {
                let mut plan = sisyphus_tools::plan_load(&plan_file)?;
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
                let mut plan = sisyphus_tools::plan_load(&plan_file)?;
                sisyphus_tools::plan_approve(&mut plan);
                sisyphus_tools::plan_save(&plan, &plan_file)?;
                println!("Approved plan: {}", plan.name);
            }
        }

        // ── oracle ──────────────────────────────────────
        Commands::Oracle(sub) => match sub {
            OracleCommands::AnalyzeFile { path } => {
                let analysis = sisyphus_tools::analyze_file(&path)?;
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
                let summary = sisyphus_tools::analyze_directory(&path, include.as_deref())?;
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
                        println!("  [{}] {} (line {}) — {}", issue.severity, issue.file.display(), issue.line, issue.message);
                    }
                }
            }
        }

        // ── explore ─────────────────────────────────────
        Commands::Explore(sub) => match sub {
            ExploreCommands::Search { patterns, paths, max_matches: _, context_lines, include } => {
                let root = paths.first().map(|p| p.as_path()).unwrap_or_else(|| std::path::Path::new("."));
                let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
                let results = sisyphus_tools::search_patterns(&pattern_refs, root, include.as_deref(), context_lines)?;
                for m in &results.matches {
                    println!("{}:{} — {}", m.file.display(), m.line, m.content.trim());
                }
                if results.matches.is_empty() {
                    println!("No matches found.");
                }
            }
            ExploreCommands::FindFiles { pattern, root, limit: _ } => {
                let files = sisyphus_tools::find_files(&pattern, &root, None)?;
                for f in &files {
                    println!("{}", f.display());
                }
                if files.is_empty() {
                    println!("No files matched.");
                }
            }
        }

        // ── librarian ───────────────────────────────────
        Commands::Librarian(sub) => match sub {
            LibrarianCommands::Research { query, scope, num } => {
                let scope_enum: sisyphus_tools::ResearchScope = scope.parse()
                    .map_err(|e: String| anyhow::anyhow!("{e}"))?;
                let report = sisyphus_tools::research(&query, scope_enum, num).await?;
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
                let page = sisyphus_tools::fetch_documentation(&url).await?;
                println!("Title: {}", page.title);
                println!("Content length: {} bytes", page.content_length);
                println!("--- Preview ---");
                println!("{}", page.content_preview);
            }
            LibrarianCommands::Examples { query, language } => {
                let examples = sisyphus_tools::find_examples(&query, &language).await?;
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
                let content = std::fs::read_to_string(&report_file)?;
                let findings: Vec<sisyphus_tools::ResearchFinding> = serde_json::from_str(&content)?;
                let synthesis = sisyphus_tools::synthesise(&findings, 10);
                println!("{}", synthesis);
            }
        }

        // ── momus ────────────────────────────────────────
        Commands::Momus(sub) => match sub {
            MomusCommands::ReviewPlan { plan_file } => {
                let content = std::fs::read_to_string(&plan_file)?;
                let name = plan_file.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unnamed".to_string());
                let review = sisyphus_tools::review_plan(&content, &name);
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
                let content = std::fs::read_to_string(&path)?;
                let review = sisyphus_tools::review_code(&path, &content)?;
                println!("{}", review.summary);
                if !review.comments.is_empty() {
                    println!("\n--- Issues ---");
                    for c in &review.comments {
                        if let Some(line) = c.line {
                            println!("  [{}] {}:{} — {} ({})", c.severity, c.file.as_deref().unwrap_or("?"), line, c.message, c.category);
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
                let review = sisyphus_tools::review_architecture(&text);
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
                let walker = std::fs::read_dir(&path)?;
                let mut reviews = Vec::new();
                for entry in walker {
                    let entry = entry?;
                    let file_path = entry.path();
                    if file_path.extension().map(|e| e == "rs").unwrap_or(false) {
                        if let Ok(content) = std::fs::read_to_string(&file_path) {
                            if let Ok(review) = sisyphus_tools::review_code(&file_path, &content) {
                                reviews.push(review);
                            }
                        }
                    }
                }
                let total_passed = reviews.iter().filter(|r| r.passed).count();
                let total_failed = reviews.iter().filter(|r| !r.passed).count();
                println!("Reviewed {} files: {} passed, {} failed", reviews.len(), total_passed, total_failed);
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
        }

        // ── metis ────────────────────────────────────────
        Commands::Metis(sub) => match sub {
            MetisCommands::Analyse { request, file } => {
                let text = match file {
                    Some(f) => std::fs::read_to_string(&f)?,
                    None => request.join(" "),
                };
                if text.trim().is_empty() {
                    anyhow::bail!("No request text provided. Pass inline text or use --file.");
                }
                let analysis = sisyphus_tools::analyse(&text);
                println!("=== Metis Analysis ===");
                println!("{}", analysis.summary);
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
                        println!("  [conf: {:.1}] {} — {}", i.confidence, i.description, i.rationale);
                    }
                }
            }
        }

        // ── skill ───────────────────────────────────────
        Commands::Skill(sub) => match sub {
            SkillCommands::List => {
                skill::run_list()?;
            }
            SkillCommands::Show { name } => {
                skill::run_show(&name)?;
            }
            SkillCommands::Install { path } => {
                skill::run_install(&path)?;
            }
            SkillCommands::Remove { name } => {
                skill::run_remove(&name)?;
            }
        }

        // ── lsp ─────────────────────────────────────────
        Commands::Lsp(sub) => match sub {
            LspCommands::Diagnostics { file, language_id, server, server_args, root_uri } => {
                lsp::run_diagnostics(&file, &language_id, &server, &server_args, &root_uri).await?;
            }
            LspCommands::GotoDef { file, line, col, language_id, server, server_args, root_uri } => {
                lsp::run_goto_def(&file, line, col, &language_id, &server, &server_args, &root_uri).await?;
            }
            LspCommands::FindRefs { file, line, col, language_id, server, server_args, root_uri } => {
                lsp::run_find_refs(&file, line, col, &language_id, &server, &server_args, &root_uri).await?;
            }
            LspCommands::Hover { file, line, col, language_id, server, server_args, root_uri } => {
                lsp::run_hover(&file, line, col, &language_id, &server, &server_args, &root_uri).await?;
            }
            LspCommands::Symbols { file, language_id, server, server_args, root_uri } => {
                lsp::run_symbols(&file, &language_id, &server, &server_args, &root_uri).await?;
            }
        }

        // ── delegate ────────────────────────────────────
        Commands::Delegate(sub) => match sub {
            DelegateCommands::Run { file, max_concurrent } => {
                delegate::run_tasks(file, max_concurrent).await?;
            }
            DelegateCommands::Inline { commands, max_concurrent } => {
                delegate::run_inline(&commands, max_concurrent).await?;
            }
        }

        // ── mcp ─────────────────────────────────────────
        Commands::Mcp(sub) => match sub {
            McpCommands::Info { connect } => {
                mcp::run_info(&connect).await?;
            }
            McpCommands::Call { tool, args, connect } => {
                mcp::run_call(&tool, args.as_deref(), &connect).await?;
            }
            McpCommands::Serve => {
                mcp::run_serve().await?;
            }
        }

        // ── engine ──────────────────────────────────────
        Commands::Sop(sub) => match sub {
            SopCommands::Run { sop_id } => {
                let mut client = bridge::BridgeClient::connect(&bridge_addr).await?;
                let result = client.execute_sop(&sop_id).await?;
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
        }
        Commands::Health => {
            let mut client = bridge::BridgeClient::connect(&bridge_addr).await?;
            let resp = client.health_check().await?;
            println!("Status: {}", resp.status);
            println!("Version: {}", resp.version);
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
                println!("Binary not found at {}, falling back to cargo run...", binary.display());
                std::process::Command::new("cargo")
                    .args(["run", "-p", "sisyphus-bridge", "--release"])
                    .spawn()
                    .context("Failed to start bridge daemon via cargo run")?
            };
            println!("Bridge daemon started (PID: {}). Running in background.", child.id());
            println!("Use Ctrl+C to stop the daemon.");
            std::thread::spawn(move || {
                let _ = child.wait_with_output();
            });
        }
    }

    Ok(())
}
