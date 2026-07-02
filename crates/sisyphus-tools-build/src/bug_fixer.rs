// bug_fixer.rs - Re-exports from auto_fix for API compatibility
// The main implementation is in the auto_fix module.
pub use crate::auto_fix::{AutoFixer, FixResult, FixSuggestion, ParsedError};
