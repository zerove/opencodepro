//! Local re-export module for sisyphus-tools sub-crates.
//! Replaces the `sisyphus-tools` facade crate with direct sub-crate paths.

// ── Foundation types from sisyphus-tools-core ───────────────────────
pub use sisyphus_tools_core::feedback::{Feedback, FeedbackStore};
pub use sisyphus_tools_core::hook::{
    execute_with_hooks, hooks_from_skill, AfterHookResult, BeforeHookResult, HookDef, HookEffect,
    HookPhase, HookRegistry,
};
pub use sisyphus_tools_core::{Permission, PermissionRules, ToolError, ToolResult};

// ── SCM tools ───────────────────────────────────────────────────────
pub mod hashline {
    pub use sisyphus_tools_scm::hashline::*;
}
pub use hashline::*;

pub mod git {
    pub use sisyphus_tools_scm::git::*;
}
pub use git::{
    add, branch, checkout, commit, current_branch, diff, init, last_commit_diff, log, pull, push,
    status, summary, BranchInfo, CommitEntry, FileStatus, GitCommitLog, GitStatus,
};

pub mod github {
    pub use sisyphus_tools_scm::github::*;
}
pub use sisyphus_tools_scm::github::{
    ci_environment, generate_workflow, is_ci, post_pr_comment, validate_workflow_yaml,
    write_workflow, CiConfig, GenerateOptions, WorkflowKind,
};

pub mod patch {
    pub use sisyphus_tools_scm::patch::*;
}
pub use patch::*;

// ── LLM abstraction ─────────────────────────────────────────────────
pub mod llm {
    pub use sisyphus_tools_llm::llm::*;
}
pub use llm::*;

pub mod metis {
    pub use sisyphus_tools_llm::metis::*;
}
pub use metis::*;

pub mod provider {
    pub use sisyphus_tools_llm::provider::*;
}
pub use sisyphus_tools_llm::provider::{
    build_provider_registry, AnthropicProvider, ChatMessage, ChatRole, GoogleGeminiProvider,
    LlmProvider, OpenAiCompatibleProvider, ProviderError, ProviderRegistry, ProviderResponse,
    RetryConfig,
};

pub mod model_router {
    pub use sisyphus_tools_llm::model_router::*;
}
pub use model_router::*;

pub mod circuit_breaker {
    pub use sisyphus_tools_llm::circuit_breaker::*;
}
pub use circuit_breaker::*;

// ── LLM tools (librarian, diagnose, oracle, momus, etc.) ────────────
pub mod librarian {
    pub use sisyphus_tools_llm_tools::librarian::*;
}
pub use librarian::{
    fetch_documentation, find_examples, now_iso, research, synthesise, CodeExample, DocPage,
    ResearchFinding, ResearchReport, ResearchScope,
};

pub mod diagnose {
    pub use sisyphus_tools_llm_tools::diagnose::*;
}
pub use diagnose::*;

pub mod oracle {
    pub use sisyphus_tools_llm_tools::oracle::*;
}
pub use oracle::{
    analyze_directory, analyze_file, code_review, CodeIssue, FileAnalysis, FunctionInfo,
    IssueSeverity, ModuleSummary,
};

pub mod scribe {
    pub use sisyphus_tools_llm_tools::scribe::*;
}
pub use scribe::*;

pub mod security {
    pub use sisyphus_tools_llm_tools::security::*;
}

pub mod pr {
    pub use sisyphus_tools_llm_tools::pr::*;
}
pub use pr::*;

pub mod momus {
    pub use sisyphus_tools_llm_tools::momus::*;
}
pub use momus::{
    review_architecture, review_code, review_code_batch, review_plan, MomusReview, ReviewComment,
    ReviewSeverity, ReviewType,
};

pub mod meta_learner {
    pub use sisyphus_tools_llm_tools::meta_learner::*;
}

// ── Build tools ─────────────────────────────────────────────────────
pub mod build {
    pub use sisyphus_tools_build::build::*;
}
pub use build::*;

pub mod lsp {
    pub use sisyphus_tools_build::lsp::*;
}
pub use lsp::{
    extract_hover_text, severity_name, to_file_uri, LspClient, LspCompletionItem, LspDiagnostic,
    LspDocumentSymbol, LspError, LspHover, LspLocation, LspPosition, LspRange, LspResult,
};

pub mod architecture {
    pub use sisyphus_tools_build::architecture::*;
}
pub mod auto_doc {
    pub use sisyphus_tools_build::auto_doc::*;
}
pub mod auto_fix {
    pub use sisyphus_tools_build::auto_fix::*;
}
pub mod benchmark {
    pub use sisyphus_tools_build::benchmark::*;
}
pub use benchmark::*;
pub mod bug_fixer {
    pub use sisyphus_tools_build::bug_fixer::*;
}
pub mod test_gen {
    pub use sisyphus_tools_build::test_gen::*;
}

// ── Agent tools ─────────────────────────────────────────────────────
pub mod agent_registry {
    pub use sisyphus_tools_agent::agent_registry::*;
}
pub use agent_registry::*;

pub mod delegate {
    pub use sisyphus_tools_agent::delegate::*;
}
pub use delegate::{work_unit, DelegateEvent, WorkOrchestrator, WorkReport, WorkStatus, WorkUnit};

pub mod developer {
    pub use sisyphus_tools_agent::developer::*;
}
pub use developer::{
    format_report, DevAction, DevCheckpoint, DevOverallStatus, DevStep, Developer, DevelopmentPlan,
    DevelopmentReport, StepResult, StepStatus, VerificationMethod,
};

pub mod project_rules {
    pub use sisyphus_tools_agent::project_rules::*;
}
pub use project_rules::*;

pub mod team {
    pub use sisyphus_tools_agent::team::*;
}
pub use team::*;

// ── System tools ────────────────────────────────────────────────────
pub mod bash {
    pub use sisyphus_tools_system::bash::*;
}
pub use bash::*;

pub mod config_store {
    pub use sisyphus_tools_system::config_store::*;
}
pub use config_store::*;

pub mod file {
    pub use sisyphus_tools_system::file::*;
}
pub use file::*;

pub mod explore {
    pub use sisyphus_tools_system::explore::*;
}
pub use explore::{
    find_files, gather_context, search_patterns, ExploreMatch, ExploreResult, FileContext,
};

pub mod web {
    pub use sisyphus_tools_system::web::*;
}
pub use web::{
    build_direct_client, extract_host, extract_port, webfetch, websearch, FetchResult, SearchResult,
};

pub mod permissions {
    pub use sisyphus_tools_system::permissions::*;
}
pub use permissions::*;

pub mod scaffold {
    pub use sisyphus_tools_system::scaffold::*;
}
pub use scaffold::*;

pub mod wisdom {
    pub use sisyphus_tools_system::wisdom::*;
}
pub use wisdom::*;

pub mod agent_channel {
    pub use sisyphus_tools_system::agent_channel::*;
}
pub mod dep_manager {
    pub use sisyphus_tools_system::dep_manager::*;
}

// ── Session tools ───────────────────────────────────────────────────
pub mod plan {
    pub use sisyphus_tools_session::plan::*;
}
pub use plan::{
    plan_approve, plan_cancel, plan_create, plan_from_requirements, plan_load, plan_review,
    plan_save, Plan, PlanStatus, PlanStep,
};

pub mod precommit {
    pub use sisyphus_tools_session::precommit::*;
}
pub use precommit::{
    has_precommit, install_precommit, is_sisyphus_hook, read_precommit, uninstall_precommit,
    PrecommitConfig,
};

pub mod session {
    pub use sisyphus_tools_session::session::*;
}
pub use session::{
    session_add_message, session_clear_checkpoint, session_compress, session_continue,
    session_create, session_delete, session_estimate_token_count, session_export,
    session_export_html, session_fork, session_get, session_import, session_list,
    session_load_checkpoint, session_rename, session_save_checkpoint, session_undo,
    CompressStrategy, Message, Session, SessionCheckpoint,
};

pub mod skill {
    pub use sisyphus_tools_session::skill::*;
}
pub use skill::{
    install_skill, remove_skill, render_instructions, Skill, SkillLoader, SkillSource, TriggerDef,
};

pub mod todo {
    pub use sisyphus_tools_session::todo::*;
}
pub use todo::*;

// ── MCP tools ───────────────────────────────────────────────────────
pub mod mcp_tools {
    pub use sisyphus_tools_mcp::mcp::*;
}
pub use mcp_tools::*;

pub mod tool_registry {
    pub use sisyphus_tools_mcp::tool_registry::*;
}
pub use tool_registry::*;

pub mod custom_tool {
    pub use sisyphus_tools_mcp::custom_tool::*;
}

// ── Executor tools ──────────────────────────────────────────────────
pub mod executor {
    pub use sisyphus_tools_executor::executor::*;
}
pub use executor::*;

pub mod executor_mcp {
    pub use sisyphus_tools_executor::executor_mcp::*;
}
pub use executor_mcp::*;
