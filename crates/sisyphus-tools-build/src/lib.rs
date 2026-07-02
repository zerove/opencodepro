//! Build and development tools for the Sisyphus tool ecosystem.
//!
//! Provides build system integration, LSP client, benchmarking, test generation,
//! auto-fix, auto-doc, and architecture analysis.

pub mod architecture;
pub mod auto_doc;
pub mod auto_fix;
pub mod benchmark;
pub mod build;
pub mod bug_fixer;
pub mod lsp;
pub mod test_gen;

pub use architecture::*;
pub use auto_doc::*;
pub use auto_fix::*;
pub use benchmark::*;
pub use build::*;
pub use lsp::{
    extract_hover_text, severity_name, to_file_uri, LspClient, LspCompletionItem, LspDiagnostic,
    LspDocumentSymbol, LspError, LspHover, LspLocation, LspPosition, LspRange, LspResult,
};
pub use test_gen::*;
