#![allow(clippy::cast_precision_loss)]

// Re-export foundation types from sisyphus-tools-core
pub use sisyphus_tools_core::{ToolError, ToolResult, Permission, PermissionRules};

pub mod hashline {
    pub use sisyphus_tools_scm::hashline::*;
}
pub use hashline::*;
pub mod agent_registry {
    pub use sisyphus_tools_agent::agent_registry::*;
}
pub mod pr {
    pub use sisyphus_tools_llm_tools::pr::*;
}
pub mod bash {
    pub use sisyphus_tools_system::bash::*;
}
pub mod build {
    pub use sisyphus_tools_build::build::*;
}
pub mod config_store {
    pub use sisyphus_tools_system::config_store::*;
}
pub mod delegate {
    pub use sisyphus_tools_agent::delegate::*;
}
pub mod diagnose {
    pub use sisyphus_tools_llm_tools::diagnose::*;
}
pub use diagnose::*;
pub mod executor {
    pub use sisyphus_tools_executor::executor::*;
}
pub mod executor_mcp {
    pub use sisyphus_tools_executor::executor_mcp::*;
}
pub mod explore {
    pub use sisyphus_tools_system::explore::*;
}
pub mod feedback {
    pub use sisyphus_tools_core::feedback::*;
}
pub mod file {
    pub use sisyphus_tools_system::file::*;
}
pub mod git {
    pub use sisyphus_tools_scm::git::*;
}
pub mod hook {
    pub use sisyphus_tools_core::hook::*;
}
pub mod librarian {
    pub use sisyphus_tools_llm_tools::librarian::*;
}
pub mod llm {
    pub use sisyphus_tools_llm::llm::*;
}
pub mod lsp {
    pub use sisyphus_tools_build::lsp::*;
}
pub mod metis {
    pub use sisyphus_tools_llm::metis::*;
}
pub mod momus {
    pub use sisyphus_tools_llm_tools::momus::*;
}
pub mod oracle {
    pub use sisyphus_tools_llm_tools::oracle::*;
}
pub mod patch {
    pub use sisyphus_tools_scm::patch::*;
}
pub mod permissions {
    pub use sisyphus_tools_system::permissions::*;
}
pub mod plan {
    pub use sisyphus_tools_session::plan::*;
}
pub mod provider {
    pub use sisyphus_tools_llm::provider::*;
}
pub use sisyphus_tools_llm::provider::{
    AnthropicProvider, build_provider_registry, ChatMessage, ChatRole, GoogleGeminiProvider,
    LlmProvider, OpenAiCompatibleProvider, ProviderError, ProviderRegistry, ProviderResponse,
    RetryConfig,
};
pub mod precommit {
    pub use sisyphus_tools_session::precommit::*;
}
pub use precommit::{
    PrecommitConfig, has_precommit, install_precommit, is_sisyphus_hook, read_precommit,
    uninstall_precommit,
};
pub mod project_rules {
    pub use sisyphus_tools_agent::project_rules::*;
}
pub mod scaffold {
    pub use sisyphus_tools_system::scaffold::*;
}
pub use scaffold::*;
pub mod scribe {
    pub use sisyphus_tools_llm_tools::scribe::*;
}
pub mod security {
    pub use sisyphus_tools_llm_tools::security::*;
}
pub mod session {
    pub use sisyphus_tools_session::session::*;
}
pub mod skill {
    pub use sisyphus_tools_session::skill::*;
}
pub mod todo {
    pub use sisyphus_tools_session::todo::*;
}
pub mod web {
    pub use sisyphus_tools_system::web::*;
}

pub mod developer {
    pub use sisyphus_tools_agent::developer::*;
}
pub use developer::{
    DevAction, DevCheckpoint, DevOverallStatus, DevStep, DevelopmentPlan, DevelopmentReport,
    Developer, StepResult, StepStatus, VerificationMethod, format_report,
};
pub mod github {
    pub use sisyphus_tools_scm::github::*;
}
pub use sisyphus_tools_scm::github::{
    CiConfig, GenerateOptions, WorkflowKind, ci_environment, generate_workflow, is_ci,
    post_pr_comment, validate_workflow_yaml, write_workflow,
};
pub mod wisdom {
    pub use sisyphus_tools_system::wisdom::*;
}
pub mod agent_channel {
    pub use sisyphus_tools_system::agent_channel::*;
}
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
pub mod circuit_breaker {
    pub use sisyphus_tools_llm::circuit_breaker::*;
}
pub use circuit_breaker::*;
pub mod dep_manager {
    pub use sisyphus_tools_system::dep_manager::*;
}
pub mod mcp {
    pub use sisyphus_tools_mcp::mcp::*;
}
pub mod meta_learner {
    pub use sisyphus_tools_llm_tools::meta_learner::*;
}
pub mod model_router {
    pub use sisyphus_tools_llm::model_router::*;
}
pub use model_router::*;
pub mod team {
    pub use sisyphus_tools_agent::team::*;
}
pub mod test_gen {
    pub use sisyphus_tools_build::test_gen::*;
}
pub mod tool_registry {
    pub use sisyphus_tools_mcp::tool_registry::*;
}
pub mod custom_tool {
    pub use sisyphus_tools_mcp::custom_tool::*;
}

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
