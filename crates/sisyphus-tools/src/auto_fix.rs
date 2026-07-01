use serde::{Deserialize, Serialize};
use std::time::Instant;

/// A suggested fix for a compiler error or bug.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSuggestion {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub error_code: String,
    pub error_message: String,
    pub suggested_fix: String,
    pub confidence: f64,
}

/// Result of an auto-fix attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixResult {
    pub file: String,
    pub fixes_applied: Vec<FixSuggestion>,
    pub success: bool,
    pub verification_output: String,
    pub duration_secs: f64,
}

/// Result of parsing a rustc error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedError {
    pub error_code: String,
    pub message: String,
    pub file: Option<String>,
    pub line: u32,
    pub column: u32,
}

/// Parses rustc compiler errors and applies automated fixes.
pub struct AutoFixer;

impl AutoFixer {
    /// Parse a rustc error output into structured error entries.
    ///
    /// Handles common error formats:
    /// - `error[E0308]: mismatched types`
    /// - `error: cannot find value `foo` in this scope`
    /// - `error[E0432]: unresolved import`
    pub fn parse_errors(error_output: &str) -> Vec<ParsedError> {
        let mut errors = Vec::new();

        for line in error_output.lines() {
            let trimmed = line.trim();

            // Pattern: error[EXXXX]: message
            if let Some(rest) = trimmed.strip_prefix("error[") {
                if let Some(close_bracket) = rest.find(']') {
                    let error_code = rest[..close_bracket].to_string();
                    let message = rest[close_bracket + 1..]
                        .trim_start_matches(':')
                        .trim()
                        .to_string();
                    errors.push(ParsedError {
                        error_code,
                        message,
                        file: None,
                        line: 0,
                        column: 0,
                    });
                }
            } else if let Some(message) = trimmed.strip_prefix("error: ") {
                // Pattern: error: message (no error code)
                errors.push(ParsedError {
                    error_code: "E0000".into(),
                    message: message.to_string(),
                    file: None,
                    line: 0,
                    column: 0,
                });
            }

            // Extract file:line:column from error context lines
            // Pattern:   --> file.rs:42:12
            if let Some(loc) = trimmed.strip_prefix("--> ") {
                let location = loc.trim();
                if let Some(last_error) = errors.last_mut() {
                    if let Some(colon_pos) = location.rfind(':') {
                        let rest = &location[..colon_pos];
                        if let Some(second_colon) = rest.rfind(':') {
                            let col: u32 = location[colon_pos + 1..].parse().unwrap_or(0);
                            let line_str = &rest[second_colon + 1..];
                            let line: u32 = line_str.parse().unwrap_or(0);
                            let file = rest[..second_colon].to_string();
                            last_error.file = Some(file);
                            last_error.line = line;
                            last_error.column = col;
                        }
                    }
                }
            }
        }

        errors
    }

    /// Generate a fix suggestion for a parsed error.
    ///
    /// This uses simple pattern matching on error codes/messages.
    /// For complex fixes, an LLM should be used instead.
    pub fn suggest_fix(error: &ParsedError) -> Option<FixSuggestion> {
        let file = error.file.clone().unwrap_or_default();
        let code = &error.error_code;
        let msg = &error.message;

        let (suggested_fix, confidence) = match code.as_str() {
            "E0308" => (
                format!(
                    "Type mismatch at {}:{}: {}. Check the expected vs actual types and add an explicit cast or conversion.",
                    file, error.line, msg
                ),
                0.5,
            ),
            "E0425" | "E0000" if msg.contains("cannot find") => (
                format!(
                    "Cannot find symbol at {}:{}. Check spelling, import, or module path.",
                    file, error.line
                ),
                0.4,
            ),
            "E0432" => (
                format!(
                    "Unresolved import at {}:{}. Check that the module path exists and the dependency is in Cargo.toml.",
                    file, error.line
                ),
                0.6,
            ),
            "E0277" => (
                format!(
                    "Trait bound not satisfied at {}:{}. Consider adding the required trait implementation or using a different type.",
                    file, error.line
                ),
                0.4,
            ),
            "E0599" => (
                format!(
                    "No method named at {}:{}. Check for typos, visibility, or trait imports.",
                    file, error.line
                ),
                0.5,
            ),
            _ => (
                format!(
                    "Unrecognized error {} at {}:{}: {}. Manual review recommended.",
                    code, file, error.line, msg
                ),
                0.2,
            ),
        };

        Some(FixSuggestion {
            file,
            line: error.line,
            column: error.column,
            error_code: code.clone(),
            error_message: msg.clone(),
            suggested_fix,
            confidence,
        })
    }

    /// Full pipeline: parse errors, suggest fixes, and attempt to apply.
    ///
    /// Note: This version only returns suggestions without applying them.
    /// The actual application of fixes requires LLM integration for precision.
    pub fn analyze_and_suggest(error_output: &str, _source_file: &str) -> Vec<FixSuggestion> {
        let errors = Self::parse_errors(error_output);
        let mut suggestions = Vec::new();

        for err in &errors {
            if let Some(fix) = Self::suggest_fix(err) {
                suggestions.push(fix);
            }
        }

        suggestions
    }

    /// Attempt a full fix-and-verify cycle.
    ///
    /// 1. Parse errors
    /// 2. Generate suggestions
    /// 3. Run verification command
    /// 4. Report results
    pub async fn fix_and_verify(error_output: &str, source_file: &str) -> FixResult {
        let start = Instant::now();

        let suggestions = Self::analyze_and_suggest(error_output, source_file);

        // Attempt to run cargo check as verification
        let verification = Self::run_cargo_check(source_file).await;

        FixResult {
            file: source_file.to_string(),
            fixes_applied: suggestions,
            success: verification.is_empty(),
            verification_output: verification,
            duration_secs: start.elapsed().as_secs_f64(),
        }
    }

    async fn run_cargo_check(source_file: &str) -> String {
        // Try to find the Cargo.toml and run cargo check
        let manifest_path = Self::find_manifest(source_file);
        let output = tokio::process::Command::new("cargo")
            .args(["check", "--manifest-path", &manifest_path, "-q"])
            .output()
            .await;

        match output {
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                if out.status.success() {
                    String::new()
                } else {
                    stderr
                }
            }
            Err(e) => format!("Failed to run cargo check: {e}"),
        }
    }

    fn find_manifest(source_file: &str) -> String {
        let path = std::path::Path::new(source_file);
        // Walk up to find Cargo.toml
        let mut current = path.parent().unwrap_or(std::path::Path::new("."));
        loop {
            let manifest = current.join("Cargo.toml");
            if manifest.exists() {
                return manifest.to_string_lossy().to_string();
            }
            match current.parent() {
                Some(p) => current = p,
                None => return "Cargo.toml".to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_e0308_error() {
        let output = "error[E0308]: mismatched types
  --> src/main.rs:42:12
   |
42 |     let x: i32 = \"hello\";
   |                  ^^^^^^^ expected i32, found &str
";
        let errors = AutoFixer::parse_errors(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].error_code, "E0308");
        assert!(errors[0].message.contains("mismatched types"));
        assert_eq!(errors[0].file.as_deref(), Some("src/main.rs"));
        assert_eq!(errors[0].line, 42);
        assert_eq!(errors[0].column, 12);
    }

    #[test]
    fn test_parse_error_without_code() {
        let output = "error: cannot find value `foo` in this scope
  --> src/lib.rs:10:5
";
        let errors = AutoFixer::parse_errors(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].error_code, "E0000");
        assert!(errors[0].message.contains("cannot find value"));
        assert_eq!(errors[0].line, 10);
    }

    #[test]
    fn test_parse_multiple_errors() {
        let output = "error[E0308]: mismatched types
  --> src/main.rs:1:1
error[E0432]: unresolved import
  --> src/lib.rs:5:10
";
        let errors = AutoFixer::parse_errors(output);
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn test_parse_empty_output() {
        let errors = AutoFixer::parse_errors("");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_suggest_fix_known_error() {
        let err = ParsedError {
            error_code: "E0308".into(),
            message: "mismatched types".into(),
            file: Some("src/main.rs".into()),
            line: 10,
            column: 5,
        };
        let fix = AutoFixer::suggest_fix(&err).unwrap();
        assert!(fix.suggested_fix.contains("Type mismatch"));
        assert_eq!(fix.confidence, 0.5);
    }

    #[test]
    fn test_suggest_fix_unknown_error() {
        let err = ParsedError {
            error_code: "E9999".into(),
            message: "some weird error".into(),
            file: None,
            line: 0,
            column: 0,
        };
        let fix = AutoFixer::suggest_fix(&err).unwrap();
        assert_eq!(fix.confidence, 0.2);
        assert!(fix.suggested_fix.contains("Unrecognized error"));
    }

    #[test]
    fn test_analyze_and_suggest_pipeline() {
        let output = "error[E0308]: mismatched types
  --> src/main.rs:42:12
";
        let suggestions = AutoFixer::analyze_and_suggest(output, "src/main.rs");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].error_code, "E0308");
    }

    #[test]
    fn test_find_manifest_from_file() {
        // Just test it doesn't panic - actual result depends on filesystem
        let manifest = AutoFixer::find_manifest("src/main.rs");
        assert!(!manifest.is_empty());
    }
}
