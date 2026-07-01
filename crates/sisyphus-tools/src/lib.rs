#![allow(clippy::cast_precision_loss)]
use thiserror::Error;

pub mod hashline;
pub use hashline::*;
pub mod agent_registry;
pub mod pr;
pub mod bash;
pub mod build;
pub mod config_store;
pub mod delegate;
pub mod diagnose;
pub use diagnose::*;
pub mod executor;
pub mod executor_mcp;
pub mod explore;
pub mod feedback;
pub mod file;
pub mod git;
pub mod hook;
pub mod librarian;
pub mod llm;
pub mod lsp;
pub mod metis;
pub mod momus;
pub mod oracle;
pub mod patch;
pub mod permissions;
pub mod plan;
pub mod provider;
pub use provider::{
    AnthropicProvider, build_provider_registry, ChatMessage, ChatRole, GoogleGeminiProvider,
    LlmProvider, OpenAiCompatibleProvider, ProviderError, ProviderRegistry, ProviderResponse,
    RetryConfig,
};
pub mod precommit;
pub use precommit::{
    PrecommitConfig, has_precommit, install_precommit, is_sisyphus_hook, read_precommit,
    uninstall_precommit,
};
pub mod project_rules;
pub mod scaffold;
pub use scaffold::*;
pub mod scribe;
pub mod security;
pub mod session;
pub mod skill;
pub mod todo;
pub mod web;

pub mod developer;
pub use developer::{
    DevAction, DevCheckpoint, DevOverallStatus, DevStep, DevelopmentPlan, DevelopmentReport,
    Developer, StepResult, StepStatus, VerificationMethod, format_report,
};
pub mod github;
pub use github::{
    CiConfig, GenerateOptions, WorkflowKind, ci_environment, generate_workflow, is_ci,
    post_pr_comment, validate_workflow_yaml, write_workflow,
};
pub mod bridge;
pub use bridge::*;
pub mod wisdom;
pub mod agent_channel;
pub mod architecture;
pub mod auto_doc;
pub mod auto_fix;
pub mod benchmark;
pub use benchmark::*;
pub mod bug_fixer;
pub mod circuit_breaker;
pub use circuit_breaker::*;
pub mod dep_manager;
pub mod mcp;
pub mod meta_learner;
pub mod model_router;
pub use model_router::*;
pub mod team;
pub mod test_gen;
pub mod tool_registry;
pub mod custom_tool;


#[derive(Error, Debug)]
pub enum ToolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Generic error: {0}")]
    Generic(String),
    #[error("Path not found: {0}")]
    NotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Edit failed: no match for old string")]
    EditNoMatch,
    #[error("Edit failed: multiple matches")]
    EditAmbiguous,
    #[error("Glob pattern error: {0}")]
    GlobPattern(String),
    #[error("Grep error: {0}")]
    GrepError(String),
    #[error("Operation timed out: {0}")]
    Timeout(String),
    #[error("Web error: {0}")]
    WebError(String),
    #[error("Todo error: {0}")]
    TodoError(String),
    #[error("Bash command failed (exit {code}): {stderr}")]
    CommandFailed {
        code: i32,
        stdout: String,
        stderr: String,
    },
}

pub type ToolResult<T> = Result<T, ToolError>;

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        ToolError::Generic(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    Allow,
    Deny,
    Ask,
}

pub type PermissionRules = Vec<(String, Permission)>;

pub use pr::*;
pub use wisdom::*;
pub use agent_registry::*;
pub use bash::*;
pub use build::*;
pub use config_store::*;
pub use delegate::{
    DelegateEvent, WorkOrchestrator, WorkReport, WorkStatus, WorkUnit, work_unit,
};
pub use executor::*;
pub use executor_mcp::*;
pub use explore::{
    ExploreMatch, ExploreResult, FileContext, find_files, gather_context, search_patterns,
};
pub use feedback::{Feedback, FeedbackStore};
pub use file::*;
pub use git::{
    BranchInfo, CommitEntry, FileStatus, GitCommitLog, GitStatus, add, branch, checkout, commit,
    current_branch, diff, init, last_commit_diff, log, pull, push, status, summary,
};
pub use hook::{
    AfterHookResult, BeforeHookResult, HookDef, HookEffect, HookPhase, HookRegistry,
    execute_with_hooks, hooks_from_skill,
};
pub use librarian::{
    fetch_documentation, find_examples, now_iso, research, synthesise, CodeExample, DocPage,
    ResearchFinding, ResearchReport, ResearchScope,
};
pub use llm::*;
pub use lsp::{
    extract_hover_text, severity_name, to_file_uri, LspClient, LspCompletionItem, LspDiagnostic,
    LspDocumentSymbol, LspError, LspHover, LspLocation, LspPosition, LspRange, LspResult,
};
pub use mcp::*;
pub use metis::*;
pub use momus::{
    review_architecture, review_code, review_code_batch, review_plan, MomusReview,
    ReviewComment, ReviewSeverity, ReviewType,
};
pub use oracle::{
    analyze_file, analyze_directory, code_review, CodeIssue, FileAnalysis, FunctionInfo,
    IssueSeverity, ModuleSummary,
};
pub use patch::*;
pub use permissions::*;
pub use plan::{
    Plan, PlanStatus, PlanStep, plan_approve, plan_cancel, plan_create, plan_from_requirements,
    plan_load, plan_review, plan_save,
};
pub use project_rules::*;
pub use scribe::*;
pub use security::*;
pub use session::{
    CompressStrategy, Message, Session, SessionCheckpoint,
    session_add_message, session_clear_checkpoint, session_compress, session_continue,
    session_create, session_delete, session_estimate_token_count, session_export,
    session_export_html, session_fork, session_get, session_import, session_list,
    session_load_checkpoint, session_rename, session_save_checkpoint, session_undo,
};
pub use skill::{
    install_skill, remove_skill, render_instructions, Skill, SkillLoader, SkillSource, TriggerDef,
};
pub use team::*;
pub use todo::*;
pub use tool_registry::*;
pub use web::{
    build_direct_client, extract_host, extract_port, webfetch, websearch, FetchResult, SearchResult,
};
