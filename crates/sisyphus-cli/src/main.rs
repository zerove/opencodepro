use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use serde_json::json;
use sisyphus_tools::{Permission, PermissionChecker, PermissionRules, TodoStatus, TodoPriority};

#[derive(Parser)]
#[command(name = "sisyphus", about = "Sisyphus AI Agent Platform")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(short = 'c', long, global = true, help = "Path to permission rules JSON file")]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// List all available tools
    List,
    /// Read file content
    Read {
        path: PathBuf,
        #[arg(short = 'o', long)]
        offset: Option<usize>,
        #[arg(short = 'l', long)]
        limit: Option<usize>,
    },
    /// Write content to file
    Write {
        path: PathBuf,
        content: String,
    },
    /// Edit file by replacing text
    Edit {
        path: PathBuf,
        old_string: String,
        new_string: String,
    },
    /// Search files by glob pattern
    Glob {
        pattern: String,
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Search file contents by regex
    Grep {
        pattern: String,
        path: PathBuf,
        #[arg(long)]
        include: Option<String>,
    },
    /// Execute bash command
    Exec {
        command: String,
        #[arg(long)]
        workdir: Option<PathBuf>,
        #[arg(long, default_value = "120000")]
        timeout: u64,
    },
    /// Search the web
    Search {
        query: String,
        #[arg(short = 'n', long, default_value = "5")]
        num_results: usize,
    },
    /// Fetch a URL
    Fetch {
        url: String,
    },
    /// Manage todo items
    #[command(subcommand)]
    Todo(TodoCommand),
    /// Manage sessions
    #[command(subcommand)]
    Session(SessionCommand),
    /// Explore codebase - multi-pattern search and context gathering
    #[command(subcommand)]
    Explore(ExploreCommand),
    /// Oracle code analysis - read-only architecture consultant
    #[command(subcommand)]
    Oracle(OracleCommand),
    /// Plan mode - create, review and export plans (read-only)
    #[command(subcommand)]
    Plan(PlanCommand),
    /// Delegate work to sub-agents (parallel task execution)
    #[command(subcommand)]
    Delegate(DelegateCommand),
    /// MCP client operations
    #[command(subcommand)]
    Mcp(McpCommand),
}

#[derive(Subcommand)]
enum McpCommand {
    /// List tools from a stdio-based MCP server
    ListStdio {
        /// The command to execute (program + args, e.g. "npx @modelcontextprotocol/server-filesystem /tmp")
        command: String,
    },
    /// Call a tool on a stdio-based MCP server
    CallStdio {
        /// The command to execute (program + args)
        command: String,
        /// Tool name to call
        tool: String,
        /// JSON arguments for the tool (optional)
        args: Option<String>,
    },
    /// List tools from an HTTP-based MCP server
    ListHttp {
        /// Server URL (e.g. "http://localhost:8080/mcp")
        url: String,
    },
    /// Call a tool on an HTTP-based MCP server
    CallHttp {
        /// Server URL
        url: String,
        /// Tool name to call
        tool: String,
        /// JSON arguments for the tool (optional)
        args: Option<String>,
    },
}

#[derive(Subcommand)]
enum TodoCommand {
    /// Add a new todo item
    Add {
        content: String,
        #[arg(short = 'p', long, default_value = "medium")]
        priority: String,
    },
    /// List todo items
    List {
        #[arg(short = 's', long)]
        status: Option<String>,
    },
    /// Update a todo item
    Update {
        id: String,
        #[arg(short = 'c', long)]
        content: Option<String>,
        #[arg(short = 's', long)]
        status: Option<String>,
        #[arg(short = 'p', long)]
        priority: Option<String>,
    },
    /// Delete a todo item
    Delete {
        id: String,
    },
}

#[derive(Subcommand)]
enum OracleCommand {
    /// Analyze a file or directory (functions, imports, structure)
    Analyze {
        /// Path to file or directory
        path: PathBuf,
    },
    /// Review code for potential issues
    Review {
        /// Files or directories to review
        paths: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PlanCommand {
    /// Create a new plan from requirements text
    Create {
        /// Plan name
        name: String,
        /// Requirements / description text for the plan
        requirements: String,
        /// Output file path (optional)
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// Show plan details from a file
    Show {
        /// Path to plan JSON file
        file: PathBuf,
    },
    /// Review a plan for issues
    Review {
        /// Path to plan JSON file
        file: PathBuf,
    },
    /// Approve a plan
    Approve {
        /// Path to plan JSON file
        file: PathBuf,
    },
    /// Export a plan as JSON
    Export {
        /// Path to plan JSON file
        file: PathBuf,
        /// Output file path (defaults to stdout)
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DelegateCommand {
    /// Run a single delegated task (executes a bash command)
    Run {
        /// Name for this task
        name: String,
        /// Bash command to execute
        command: String,
        /// Timeout in milliseconds
        #[arg(long, default_value = "120000")]
        timeout: u64,
    },
    /// Run multiple tasks in parallel from a JSON file
    Batch {
        /// Path to JSON file with task definitions
        file: PathBuf,
        /// Max concurrent tasks (0 = unlimited)
        #[arg(long, default_value = "4")]
        concurrent: usize,
    },
}

#[derive(Subcommand)]
enum ExploreCommand {
    /// Search multiple patterns across the codebase
    Search {
        /// One or more regex patterns to search for
        patterns: Vec<String>,
        /// Root directory to search in
        #[arg(long = "root", default_value = ".")]
        root: PathBuf,
        /// File filter (regex, e.g. "\.rs$")
        #[arg(long = "include")]
        include: Option<String>,
        /// Number of context lines before/after each match
        #[arg(long = "context", default_value = "2")]
        context: usize,
    },
    /// Gather full context from specific files
    Context {
        /// File paths to read
        paths: Vec<PathBuf>,
    },
    /// Find files by glob + optional content filter
    Find {
        /// Glob pattern (e.g. "**/*.rs")
        pattern: String,
        /// Root directory
        #[arg(long = "root", default_value = ".")]
        root: PathBuf,
        /// Optional content regex filter
        #[arg(long = "content")]
        content: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionCommand {
    /// Create a new session
    New {
        name: String,
    },
    /// List all sessions
    List,
    /// Show session details
    Show {
        id: String,
    },
    /// Delete a session
    Delete {
        id: String,
    },
    /// Rename a session
    Rename {
        id: String,
        name: String,
    },
    /// Add a message to a session
    AddMessage {
        id: String,
        role: String,
        content: String,
    },
}

fn tool_list() -> serde_json::Value {
    json!({
        "ok": true,
        "tools": [
            { "name": "read",     "description": "Read file content",              "args": ["path", "offset?", "limit?"] },
            { "name": "write",    "description": "Write content to file",          "args": ["path", "content"] },
            { "name": "edit",     "description": "Edit file by replacing text",    "args": ["path", "old_string", "new_string"] },
            { "name": "glob",     "description": "Search files by glob pattern",   "args": ["pattern", "path?"] },
            { "name": "grep",     "description": "Search file contents by regex",  "args": ["pattern", "path", "include?"] },
            { "name": "exec",     "description": "Execute bash command",           "args": ["command", "workdir?", "timeout?"] },
            { "name": "search",   "description": "Search the web",                 "args": ["query", "num_results?"] },
            { "name": "fetch",    "description": "Fetch a URL",                    "args": ["url"] },
            { "name": "todo",     "description": "Manage todo items (add/list/update/delete)", "args": ["subcommand", "..."] },
        ]
    })
}

fn check_tool(checker: &PermissionChecker, tool_name: &str) -> Result<(), serde_json::Value> {
    checker.require(tool_name).map_err(|_| {
        json!({ "error": format!("permission denied: {}", tool_name) })
    })
}

fn load_rules(path: Option<&PathBuf>) -> PermissionRules {
    let Some(path) = path else { return vec![] };
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let raw: Vec<Vec<String>> = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    raw.into_iter().filter_map(|pair| {
        if pair.len() != 2 { return None; }
        let perm = match pair[1].as_str() {
            "allow" => Permission::Allow,
            "deny"  => Permission::Deny,
            "ask"   => Permission::Ask,
            _ => return None,
        };
        Some((pair[0].clone(), perm))
    }).collect()
}

fn parse_status(s: Option<String>) -> Result<Option<TodoStatus>, serde_json::Value> {
    match s {
        None => Ok(None),
        Some(v) => v.parse::<TodoStatus>().map(Some).map_err(|e|
            json!({ "error": e.to_string() })
        ),
    }
}

fn parse_priority(s: Option<String>) -> Result<Option<TodoPriority>, serde_json::Value> {
    match s {
        None => Ok(None),
        Some(v) => v.parse::<TodoPriority>().map(Some).map_err(|e|
            json!({ "error": e.to_string() })
        ),
    }
}

fn session_dir() -> PathBuf {
    std::env::var("SISYPHUS_SESSION_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(base).join(".sisyphus").join("sessions.json")
        })
}

fn todo_dir() -> PathBuf {
    std::env::var("SISYPHUS_TODO_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(base).join(".sisyphus").join("todos.json")
        })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .json()
        .init();

    let result = run().await;

    println!("{}", result);

    if result["error"].is_string() {
        std::process::exit(1);
    }
}

async fn run() -> serde_json::Value {
    let cli = Cli::parse();

    let rules = load_rules(cli.config.as_ref());
    let checker = PermissionChecker::new(rules);

    match cli.command {
        Command::List => tool_list(),

        Command::Read { path, offset, limit } => {
            let _ = check_tool(&checker, "read");
            if offset.is_some() || limit.is_some() {
                match sisyphus_tools::read_lines(&path, offset.unwrap_or(0), limit.unwrap_or(usize::MAX)) {
                    Ok(data) => json!({ "ok": true, "data": data }),
                    Err(e) => json!({ "error": e.to_string() }),
                }
            } else {
                match sisyphus_tools::read(&path) {
                    Ok(data) => json!({ "ok": true, "data": data }),
                    Err(e) => json!({ "error": e.to_string() }),
                }
            }
        }
        Command::Write { path, content } => {
            let _ = check_tool(&checker, "write");
            match sisyphus_tools::write(&path, &content) {
                Ok(_) => json!({ "ok": true }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Command::Edit { path, old_string, new_string } => {
            let _ = check_tool(&checker, "edit");
            match sisyphus_tools::edit(&path, &old_string, &new_string) {
                Ok(_) => json!({ "ok": true }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Command::Glob { pattern, path } => {
            let _ = check_tool(&checker, "glob");
            match sisyphus_tools::glob(&pattern, &path) {
                Ok(files) => json!({ "ok": true, "files": files }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Command::Grep { pattern, path, include } => {
            let _ = check_tool(&checker, "grep");
            match sisyphus_tools::grep(&pattern, &path, include.as_deref()) {
                Ok(matches) => {
                    let matches_json: Vec<_> = matches
                        .into_iter()
                        .map(|m| json!({ "file": m.file, "line": m.line, "content": m.content }))
                        .collect();
                    json!({ "ok": true, "matches": matches_json })
                }
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Command::Exec { command, workdir, timeout } => {
            let _ = check_tool(&checker, "exec");
            match sisyphus_tools::execute(&command, workdir.as_deref(), timeout).await {
                Ok(output) => json!({
                    "ok": true,
                    "data": {
                        "stdout": output.stdout,
                        "stderr": output.stderr,
                        "exit_code": output.exit_code,
                    }
                }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Command::Search { query, num_results } => {
            let _ = check_tool(&checker, "search");
            match sisyphus_tools::websearch(&query, num_results).await {
                Ok(results) => {
                    let items: Vec<_> = results.into_iter()
                        .map(|r| json!({ "title": r.title, "url": r.url, "snippet": r.snippet }))
                        .collect();
                    json!({ "ok": true, "results": items })
                }
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Command::Fetch { url } => {
            let _ = check_tool(&checker, "fetch");
            match sisyphus_tools::webfetch(&url).await {
                Ok(result) => json!({ "ok": true, "url": result.url, "content": result.content, "content_type": result.content_type }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Command::Todo(cmd) => {
            let _ = check_tool(&checker, "todo");
            let db = todo_dir();
            match cmd {
                TodoCommand::Add { content, priority } => {
                    let prio = match priority.parse::<TodoPriority>() {
                        Ok(p) => p,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    match sisyphus_tools::todo_add(&db, &content, prio) {
                        Ok(item) => json!({ "ok": true, "todo": {
                            "id": item.id, "content": item.content,
                            "status": item.status.to_string(), "priority": item.priority.to_string(),
                            "created_at": item.created_at,
                        }}),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                TodoCommand::List { status } => {
                    let filter = match parse_status(status) {
                        Err(e) => return e,
                        Ok(f) => f,
                    };
                    match sisyphus_tools::todo_list(&db, filter) {
                        Ok(items) => {
                            let list: Vec<_> = items.into_iter()
                                .map(|i| json!({
                                    "id": i.id, "content": i.content,
                                    "status": i.status.to_string(), "priority": i.priority.to_string(),
                                    "created_at": i.created_at,
                                }))
                                .collect();
                            json!({ "ok": true, "todos": list })
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                TodoCommand::Update { id, content, status, priority } => {
                    let status = match parse_status(status) {
                        Err(e) => return e,
                        Ok(s) => s,
                    };
                    let priority = match parse_priority(priority) {
                        Err(e) => return e,
                        Ok(p) => p,
                    };
                    match sisyphus_tools::todo_update(&db, &id, content.as_deref(), status, priority) {
                        Ok(item) => json!({ "ok": true, "todo": {
                            "id": item.id, "content": item.content,
                            "status": item.status.to_string(), "priority": item.priority.to_string(),
                            "updated_at": item.updated_at,
                        }}),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                TodoCommand::Delete { id } => {
                    match sisyphus_tools::todo_delete(&db, &id) {
                        Ok(_) => json!({ "ok": true }),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
            }
        }
        Command::Mcp(cmd) => {
            let _ = check_tool(&checker, "mcp");
            match cmd {
                McpCommand::ListStdio { command } => {
                    let parts: Vec<&str> = command.split_whitespace().collect();
                    if parts.is_empty() {
                        return json!({ "error": "empty command" });
                    }
                    let transport = match sisyphus_tools::StdioTransport::spawn(parts[0], &parts[1..]).await {
                        Ok(t) => t,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    let mut client = sisyphus_tools::McpClient::new(Box::new(transport));
                    if let Err(e) = client.initialize("sisyphus-cli", "0.1.0").await {
                        return json!({ "error": e.to_string() });
                    }
                    let tools = match client.list_tools().await {
                        Ok(t) => t,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    let tools_json: Vec<_> = tools.into_iter()
                        .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema }))
                        .collect();
                    let _ = client.close().await;
                    json!({ "ok": true, "server_name": "stdio", "tools": tools_json })
                }
                McpCommand::CallStdio { command, tool, args } => {
                    let parts: Vec<&str> = command.split_whitespace().collect();
                    if parts.is_empty() {
                        return json!({ "error": "empty command" });
                    }
                    let transport = match sisyphus_tools::StdioTransport::spawn(parts[0], &parts[1..]).await {
                        Ok(t) => t,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    let mut client = sisyphus_tools::McpClient::new(Box::new(transport));
                    if let Err(e) = client.initialize("sisyphus-cli", "0.1.0").await {
                        return json!({ "error": e.to_string() });
                    }
                    let arguments = args.as_ref()
                        .and_then(|a| serde_json::from_str(a).ok());
                    let result = match client.call_tool(&tool, arguments).await {
                        Ok(r) => r,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    let _ = client.close().await;
                    let content_json: Vec<_> = result.content.into_iter()
                        .map(|c| json!(c))
                        .collect();
                    json!({ "ok": true, "is_error": result.is_error.unwrap_or(false), "content": content_json })
                }
                McpCommand::ListHttp { url } => {
                    let transport = sisyphus_tools::HttpTransport::new(&url);
                    let mut client = sisyphus_tools::McpClient::new(Box::new(transport));
                    if let Err(e) = client.initialize("sisyphus-cli", "0.1.0").await {
                        return json!({ "error": e.to_string() });
                    }
                    let tools = match client.list_tools().await {
                        Ok(t) => t,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    let tools_json: Vec<_> = tools.into_iter()
                        .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema }))
                        .collect();
                    json!({ "ok": true, "server_url": url, "tools": tools_json })
                }
                McpCommand::CallHttp { url, tool, args } => {
                    let transport = sisyphus_tools::HttpTransport::new(&url);
                    let mut client = sisyphus_tools::McpClient::new(Box::new(transport));
                    if let Err(e) = client.initialize("sisyphus-cli", "0.1.0").await {
                        return json!({ "error": e.to_string() });
                    }
                    let arguments = args.as_ref()
                        .and_then(|a| serde_json::from_str(a).ok());
                    let result = match client.call_tool(&tool, arguments).await {
                        Ok(r) => r,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    let content_json: Vec<_> = result.content.into_iter()
                        .map(|c| json!(c))
                        .collect();
                    json!({ "ok": true, "is_error": result.is_error.unwrap_or(false), "content": content_json })
                }
            }
        }
        Command::Explore(cmd) => {
            let _ = check_tool(&checker, "explore");
            match cmd {
                ExploreCommand::Search { patterns, root, include, context } => {
                    let str_patterns: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
                    match sisyphus_tools::search_patterns(&str_patterns, &root, include.as_deref(), context) {
                        Ok(result) => {
                            let matches_json: Vec<_> = result.matches.into_iter()
                                .map(|m| json!({
                                    "file": m.file.to_string_lossy(),
                                    "line": m.line,
                                    "content": m.content,
                                    "before": m.before,
                                    "after": m.after,
                                    "pattern_index": m.pattern_index,
                                }))
                                .collect();
                            json!({ "ok": true, "matches": matches_json, "files_scanned": result.files_scanned, "total_matches": result.total_matches })
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                ExploreCommand::Context { paths } => {
                    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
                    match sisyphus_tools::gather_context(&refs) {
                        Ok(contexts) => {
                            let ctx_json: Vec<_> = contexts.into_iter()
                                .map(|c| json!({
                                    "path": c.path.to_string_lossy(),
                                    "content": c.content,
                                    "line_count": c.line_count,
                                }))
                                .collect();
                            json!({ "ok": true, "files": ctx_json })
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                ExploreCommand::Find { pattern, root, content } => {
                    match sisyphus_tools::find_files(&pattern, &root, content.as_deref()) {
                        Ok(files) => {
                            let list: Vec<String> = files.into_iter()
                                .map(|p| p.to_string_lossy().into_owned())
                                .collect();
                            json!({ "ok": true, "files": list })
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
            }
        }
        Command::Oracle(cmd) => {
            let _ = check_tool(&checker, "oracle");
            match cmd {
                OracleCommand::Analyze { path } => {
                    if path.is_dir() {
                        match sisyphus_tools::analyze_directory(&path, Some(r"\.rs$")) {
                            Ok(summary) => {
                                let files_json: Vec<_> = summary.files.iter().map(|f| json!({
                                    "path": f.path.to_string_lossy(),
                                    "line_count": f.line_count,
                                    "char_count": f.char_count,
                                    "functions": f.functions.iter().map(|fn_| json!({
                                        "name": fn_.name,
                                        "line": fn_.line,
                                        "public": fn_.is_public,
                                        "async": fn_.is_async,
                                        "signature": fn_.signature,
                                    })).collect::<Vec<_>>(),
                                    "imports": f.imports,
                                    "structs": f.structs,
                                    "issues": f.issues.iter().map(|i| json!({
                                        "severity": i.severity.to_string(),
                                        "line": i.line,
                                        "message": i.message,
                                        "code": i.code,
                                    })).collect::<Vec<_>>(),
                                })).collect();
                                json!({ "ok": true, "file_count": summary.file_count, "total_lines": summary.total_lines, "files": files_json })
                            }
                            Err(e) => json!({ "error": e.to_string() }),
                        }
                    } else {
                        match sisyphus_tools::analyze_file(&path) {
                            Ok(analysis) => json!({ "ok": true, "analysis": {
                                "path": analysis.path.to_string_lossy(),
                                "line_count": analysis.line_count,
                                "char_count": analysis.char_count,
                                "functions": analysis.functions.iter().map(|fn_| json!({
                                    "name": fn_.name,
                                    "line": fn_.line,
                                    "public": fn_.is_public,
                                    "async": fn_.is_async,
                                    "signature": fn_.signature,
                                })).collect::<Vec<_>>(),
                                "imports": analysis.imports,
                                "structs": analysis.structs,
                                "issues": analysis.issues.iter().map(|i| json!({
                                    "severity": i.severity.to_string(),
                                    "line": i.line,
                                    "message": i.message,
                                    "code": i.code,
                                })).collect::<Vec<_>>(),
                            }}),
                            Err(e) => json!({ "error": e.to_string() }),
                        }
                    }
                }
                OracleCommand::Review { paths } => {
                    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
                    match sisyphus_tools::code_review(&refs) {
                        Ok(issues) => {
                            let list: Vec<_> = issues.iter().map(|i| json!({
                                "severity": i.severity.to_string(),
                                "file": i.file.to_string_lossy(),
                                "line": i.line,
                                "message": i.message,
                                "code": i.code,
                            })).collect();
                            json!({ "ok": true, "issues": list, "count": list.len() })
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
            }
        }
        Command::Plan(cmd) => {
            let _ = check_tool(&checker, "plan");
            match cmd {
                PlanCommand::Create { name, requirements, output } => {
                    let plan = sisyphus_tools::plan_from_requirements(&name, &requirements);
                    if let Some(out_path) = output {
                        if let Err(e) = sisyphus_tools::plan_save(&plan, &out_path) {
                            return json!({ "error": e.to_string() });
                        }
                        json!({ "ok": true, "message": format!("plan saved to {}", out_path.display()), "plan": {
                            "id": plan.id, "name": plan.name, "status": plan.status.to_string(),
                            "steps": plan.steps.len(),
                        }})
                    } else {
                        json!({ "ok": true, "plan": {
                            "id": plan.id, "name": plan.name, "status": plan.status.to_string(),
                            "steps": plan.steps.len(), "description": plan.description,
                        }})
                    }
                }
                PlanCommand::Show { file } => {
                    match sisyphus_tools::plan_load(&file) {
                        Ok(plan) => json!({ "ok": true, "plan": {
                            "id": plan.id, "name": plan.name, "description": plan.description,
                            "status": plan.status.to_string(),
                            "created_at": plan.created_at, "updated_at": plan.updated_at,
                            "steps": plan.steps.iter().map(|s| json!({
                                "id": s.id, "description": s.description,
                                "status": s.status.to_string(),
                                "dependencies": s.dependencies,
                                "estimated_effort": s.estimated_effort,
                            })).collect::<Vec<_>>(),
                        }}),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                PlanCommand::Review { file } => {
                    match sisyphus_tools::plan_load(&file) {
                        Ok(plan) => {
                            let comments = sisyphus_tools::plan_review(&plan);
                            json!({ "ok": true, "plan_name": plan.name, "comments": comments, "pass": comments.is_empty() })
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                PlanCommand::Approve { file } => {
                    match sisyphus_tools::plan_load(&file) {
                        Ok(mut plan) => {
                            sisyphus_tools::plan_approve(&mut plan);
                            if let Err(e) = sisyphus_tools::plan_save(&plan, &file) {
                                return json!({ "error": e.to_string() });
                            }
                            json!({ "ok": true, "plan": { "id": plan.id, "name": plan.name, "status": plan.status.to_string() } })
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                PlanCommand::Export { file, output } => {
                    match sisyphus_tools::plan_load(&file) {
                        Ok(plan) => {
                            let json_str = serde_json::to_string_pretty(&plan).unwrap_or_default();
                            if let Some(out_path) = output {
                                if let Err(e) = std::fs::write(&out_path, &json_str) {
                                    return json!({ "error": e.to_string() });
                                }
                                json!({ "ok": true, "message": format!("exported to {}", out_path.display()) })
                            } else {
                                json!({ "ok": true, "plan_json": json_str })
                            }
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
            }
        }
        Command::Delegate(cmd) => {
            let _ = check_tool(&checker, "delegate");
            match cmd {
                DelegateCommand::Run { name, command, timeout } => {
                    let unit = sisyphus_tools::work_unit("1", &name, move || {
                        let cmd = command.clone();
                        async move {
                            let output = sisyphus_tools::execute(&cmd, None, timeout).await?;
                            Ok(json!({
                                "stdout": output.stdout,
                                "stderr": output.stderr,
                                "exit_code": output.exit_code,
                            }))
                        }
                    });
                    let orchestrator = sisyphus_tools::WorkOrchestrator::new(0);
                    let reports = match orchestrator.run_all(vec![unit]).await {
                        Ok(r) => r,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    let report_list: Vec<_> = reports.iter().map(|r| json!({
                        "id": r.id, "name": r.name,
                        "status": r.status.to_string(),
                        "result": r.result,
                        "error": r.error,
                        "duration_ms": r.duration_ms,
                    })).collect();
                    json!({ "ok": true, "reports": report_list })
                }
                DelegateCommand::Batch { file, concurrent } => {
                    let content = match std::fs::read_to_string(&file) {
                        Ok(c) => c,
                        Err(e) => return json!({ "error": format!("read file: {e}") }),
                    };
                    let tasks: Vec<serde_json::Value> = match serde_json::from_str(&content) {
                        Ok(t) => t,
                        Err(e) => return json!({ "error": format!("parse JSON: {e}") }),
                    };
                    if tasks.is_empty() {
                        return json!({ "ok": true, "reports": [] });
                    }
                    let units: Vec<sisyphus_tools::WorkUnit> = tasks.into_iter().enumerate().map(|(i, task)| {
                        let id = task.get("id").and_then(|v| v.as_str()).unwrap_or(&format!("t{}", i)).to_string();
                        let name = task.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
                        let cmd = task.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let timeout = task.get("timeout").and_then(|v| v.as_u64()).unwrap_or(120000);
                        sisyphus_tools::work_unit(&id, &name, move || {
                            let cmd = cmd.clone();
                            async move {
                                let output = sisyphus_tools::execute(&cmd, None, timeout).await?;
                                Ok(json!({
                                    "stdout": output.stdout,
                                    "stderr": output.stderr,
                                    "exit_code": output.exit_code,
                                }))
                            }
                        })
                    }).collect();

                    let orchestrator = sisyphus_tools::WorkOrchestrator::new(concurrent);
                    let reports = match orchestrator.run_all(units).await {
                        Ok(r) => r,
                        Err(e) => return json!({ "error": e.to_string() }),
                    };
                    let report_list: Vec<_> = reports.iter().map(|r| json!({
                        "id": r.id, "name": r.name,
                        "status": r.status.to_string(),
                        "result": r.result,
                        "error": r.error,
                        "duration_ms": r.duration_ms,
                    })).collect();
                    json!({ "ok": true, "reports": report_list })
                }
            }
        }
        Command::Session(cmd) => {
            let _ = check_tool(&checker, "session");
            let db = session_dir();
            match cmd {
                SessionCommand::New { name } => {
                    match sisyphus_tools::session_create(&db, &name) {
                        Ok(s) => json!({ "ok": true, "session": {
                            "id": s.id, "name": s.name,
                            "created_at": s.created_at,
                            "message_count": s.message_count,
                        }}),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                SessionCommand::List => {
                    match sisyphus_tools::session_list(&db) {
                        Ok(sessions) => {
                            let list: Vec<_> = sessions.into_iter()
                                .map(|s| json!({
                                    "id": s.id, "name": s.name,
                                    "created_at": s.created_at,
                                    "updated_at": s.updated_at,
                                    "message_count": s.message_count,
                                }))
                                .collect();
                            json!({ "ok": true, "sessions": list })
                        }
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                SessionCommand::Show { id } => {
                    match sisyphus_tools::session_get(&db, &id) {
                        Ok(s) => json!({ "ok": true, "session": {
                            "id": s.id, "name": s.name,
                            "created_at": s.created_at, "updated_at": s.updated_at,
                            "message_count": s.message_count,
                            "messages": s.messages,
                            "metadata": s.metadata,
                        }}),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                SessionCommand::Delete { id } => {
                    match sisyphus_tools::session_delete(&db, &id) {
                        Ok(_) => json!({ "ok": true }),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                SessionCommand::Rename { id, name } => {
                    match sisyphus_tools::session_rename(&db, &id, &name) {
                        Ok(s) => json!({ "ok": true, "session": {
                            "id": s.id, "name": s.name,
                            "updated_at": s.updated_at,
                        }}),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                SessionCommand::AddMessage { id, role, content } => {
                    match sisyphus_tools::session_add_message(&db, &id, &role, &content) {
                        Ok(s) => json!({ "ok": true, "session": {
                            "id": s.id, "name": s.name,
                            "message_count": s.message_count,
                            "updated_at": s.updated_at,
                        }}),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
            }
        }
    }
}
