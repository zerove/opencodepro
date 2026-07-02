use std::path::PathBuf;

use clap::Subcommand;

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
