//! Build and development tools for the Sisyphus tool ecosystem.
//!
//! Provides build system integration, LSP client, benchmarking, test generation,
//! auto-fix, auto-doc, and architecture analysis.

pub mod architecture;
pub mod auto_doc;
pub mod auto_fix;
pub mod benchmark {
    pub use sisyphus_types::benchmark::*;
}
pub use sisyphus_types::benchmark::*;
pub mod bug_fixer;
pub mod build;
pub mod lsp;
pub mod test_gen;

// Targeted re-exports: only items used at root level by external consumers.
pub use lsp::{
    extract_hover_text, severity_name, to_file_uri, LspClient, LspCompletionItem, LspDiagnostic,
    LspDocumentSymbol, LspError, LspHover, LspLocation, LspPosition, LspRange, LspResult,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::Timer;
    use crate::build::{BuildOutput, Diag};

    #[test]
    fn test_timer_basic() {
        let timer = Timer::start();
        assert!(timer.elapsed_secs() >= 0.0);
    }

    #[test]
    fn test_build_output_basic() {
        let output = BuildOutput {
            errors: vec![Diag {
                file: Some("main.rs".into()),
                line: Some(1),
                col: Some(1),
                level: "error".into(),
                code: Some("E001".into()),
                message: "test error".into(),
            }],
            warnings: vec![],
            raw_stdout: "out".into(),
            raw_stderr: "err".into(),
            success: false,
        };
        assert!(!output.success);
        assert_eq!(output.errors.len(), 1);
    }

    #[test]
    fn test_lsp_diagnostic_basic() {
        let diag = LspDiagnostic {
            range: LspRange {
                start: LspPosition {
                    line: 0,
                    character: 0,
                },
                end: LspPosition {
                    line: 0,
                    character: 5,
                },
            },
            severity: Some(1),
            message: "lsp error".into(),
        };
        assert_eq!(diag.message, "lsp error");
        assert_eq!(diag.severity, Some(1));
    }
}
