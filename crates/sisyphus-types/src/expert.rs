//! Deep semantic review produced by the expert LLM tools.
//!
//! `sisyphus-core`'s reflection step wants *suggestions* — human-readable
//! strings appended to a reflection report. It does not care whether they came
//! from a file-complexity analysis or an LLM plan review; that choice belongs to
//! the tools layer.
//!
//! Previously `core` called `oracle::analyze_file` and
//! `momus::review_plan` / `review_code` directly, which kept
//! `sisyphus-tools-llm-tools` in core's production dependencies for what is
//! really a wiring concern (ADR-014). The seam below lets the heuristics live
//! next to the tools that implement them while `core` keeps only the result
//! shape it actually uses.

use std::path::PathBuf;

use async_trait::async_trait;

/// Produces expert review suggestions for one reflection round.
///
/// The returned strings are already formatted for display; `core` appends them
/// to `ReflectionReport::suggested_improvements` without inspecting them. That
/// is deliberate — it keeps severity labels and review categories out of the
/// core crate's vocabulary.
#[async_trait]
pub trait ExpertReviewer: Send + Sync {
    /// Review modified files and produced node outputs.
    ///
    /// `modified_files` are the files touched by the run; `results` pairs each
    /// SOP node name with its rendered output. Implementations decide which
    /// outputs look like plans or like code.
    async fn suggestions(
        &self,
        modified_files: Vec<PathBuf>,
        results: Vec<(String, String)>,
        llm: &dyn crate::LlmClient,
    ) -> Vec<String>;
}
