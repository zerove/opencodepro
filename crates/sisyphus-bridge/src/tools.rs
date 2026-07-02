use std::sync::Arc;

/// Register all built-in tools and agent tools from BuiltinToolExecutor.
pub(crate) fn register_builtin_tools(reg: &Arc<sisyphus_tools_mcp::tool_registry::ToolRegistry>) {
    use sisyphus_tools_mcp::tool_registry::{ToolInfo, ToolSource};
    let builtin = ToolSource::Builtin;
    // Agent tools use bare IDs (e.g. "librarian/research") matching agent_prefix "librarian/"
    // Core builtin tools use "builtin:" prefix matching router dispatch
    let tools: Vec<(&str, &str, &str)> = vec![
        // Core builtin tools (router prefix: builtin:)
        (
            "builtin:file_read",
            "file_read",
            "Read file content by path",
        ),
        (
            "builtin:file_write",
            "file_write",
            "Write content to a file",
        ),
        (
            "builtin:file_edit",
            "file_edit",
            "Replace first occurrence of old string with new string in a file",
        ),
        (
            "builtin:file_edit_all",
            "file_edit_all",
            "Replace all occurrences of old string with new string in a file",
        ),
        (
            "builtin:file_read_lines",
            "file_read_lines",
            "Read specific lines from a file with offset and limit",
        ),
        ("builtin:glob", "glob", "Find files matching a glob pattern"),
        (
            "builtin:grep",
            "grep",
            "Search file contents with a regex pattern",
        ),
        (
            "builtin:bash_exec",
            "bash_exec",
            "Execute a bash command and return output",
        ),
        (
            "builtin:web_search",
            "web_search",
            "Search the web for a query",
        ),
        ("builtin:web_fetch", "web_fetch", "Fetch content from a URL"),
        (
            "builtin:feedback/save",
            "feedback/save",
            "Save feedback for an agent response",
        ),
        (
            "builtin:feedback/get",
            "feedback/get",
            "Retrieve feedback history for an agent",
        ),
        (
            "builtin:feedback/query",
            "feedback/query",
            "Query feedback by criteria",
        ),
        // Agent tools (bare IDs match agent_prefix like "librarian/")
        (
            "librarian/research",
            "librarian/research",
            "Comprehensive web research with synthesis",
        ),
        (
            "librarian/fetch_docs",
            "librarian/fetch_docs",
            "Fetch documentation from a URL",
        ),
        (
            "librarian/examples",
            "librarian/examples",
            "Search code examples from GitHub",
        ),
        (
            "librarian/synthesise",
            "librarian/synthesise",
            "Synthesize research findings into concise report",
        ),
        (
            "metis/analyse",
            "metis/analyse",
            "Analyse requirements for ambiguity and hidden intentions",
        ),
        (
            "momus/review_plan",
            "momus/review_plan",
            "Review a plan for clarity and completeness",
        ),
        (
            "momus/review_code",
            "momus/review_code",
            "Review code changes for quality and standards",
        ),
        (
            "momus/review_code_batch",
            "momus/review_code_batch",
            "Review multiple files in batch",
        ),
        (
            "momus/review_arch",
            "momus/review_arch",
            "Review architecture design for soundness",
        ),
        (
            "scribe/generate_adr",
            "scribe/generate_adr",
            "Generate an Architecture Decision Record",
        ),
        (
            "scribe/update_docs",
            "scribe/update_docs",
            "Update documentation based on code changes",
        ),
        (
            "scribe/apply_update",
            "scribe/apply_update",
            "Apply a documentation edit",
        ),
        (
            "security/audit_code",
            "security/audit_code",
            "Audit code for security vulnerabilities",
        ),
        (
            "security/threat_model",
            "security/threat_model",
            "Model threats for an architecture",
        ),
    ];
    for (tool_id, name, desc) in tools {
        reg.register(
            tool_id,
            ToolInfo {
                name: name.into(),
                description: desc.into(),
                parameters: None,
                source: builtin.clone(),
            },
        );
    }
}

/// Register strategic planning tools from StrategicExecutor.
pub(crate) fn register_strategic_tools(reg: &Arc<sisyphus_tools_mcp::tool_registry::ToolRegistry>) {
    use sisyphus_tools_mcp::tool_registry::{ToolInfo, ToolSource};
    let source = ToolSource::Strategic;
    // Tool_id format: "strategic:<name>" matches router dispatch (prefix "strategic:")
    let tools: Vec<(&str, &str, &str)> = vec![
        (
            "strategic:next_goal",
            "next_goal",
            "Execute the next pending goal in the roadmap",
        ),
        (
            "strategic:progress",
            "progress",
            "Query overall roadmap progress and status",
        ),
        (
            "strategic:show_roadmap",
            "show_roadmap",
            "Display the full roadmap with milestones and goals",
        ),
        (
            "strategic:replan",
            "replan",
            "Re-plan the roadmap based on execution results",
        ),
        (
            "strategic:auto_plan",
            "auto_plan",
            "Automatically generate a roadmap from project description",
        ),
        (
            "strategic:goal_context",
            "goal_context",
            "Get roadmap context for a specific goal",
        ),
        (
            "strategic:reflect",
            "reflect",
            "Run strategic reflection on roadmap progress",
        ),
        (
            "strategic:predict",
            "predict",
            "Predict completion timeline and identify risks",
        ),
        (
            "strategic:project_costs",
            "project_costs",
            "Project remaining costs and duration for the project",
        ),
        (
            "strategic:bottlenecks",
            "bottlenecks",
            "Identify bottlenecks in the execution pipeline",
        ),
    ];
    for (tool_id, name, desc) in tools {
        reg.register(
            tool_id,
            ToolInfo {
                name: name.into(),
                description: desc.into(),
                parameters: None,
                source: source.clone(),
            },
        );
    }
}
