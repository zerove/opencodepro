use crate::ToolResult;
use std::path::Path;

/// A single hunk in a unified diff.
#[derive(Debug)]
struct Hunk {
    old_start: usize,
    #[allow(dead_code)] // part of unified-diff format, reserved for future use
    old_count: usize,
    #[allow(dead_code)] // part of unified-diff format, reserved for future use
    new_start: usize,
    #[allow(dead_code)] // part of unified-diff format, reserved for future use
    new_count: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug)]
enum HunkLine {
    Context(String),
    Removal(String),
    Addition(String),
}

/// Apply a unified-diff patch string to a file at `path`.
///
/// The patch format is the standard unified diff (`diff -u` style):
/// ```text
/// --- a/file.rs
/// +++ b/file.rs
/// @@ -1,5 +1,6 @@
///  context
/// -removed
/// +added
/// ```
///
/// Returns a list of files that were modified.
pub fn apply_patch(path: &Path, patch_content: &str) -> ToolResult<Vec<String>> {
    let mut modified = Vec::new();
    let hunks = parse_diff(patch_content)?;

    if hunks.is_empty() {
        return Err(crate::ToolError::Generic(
            "empty patch — no hunks found".into(),
        ));
    }

    let content = crate::read(path)?;
    let result = apply_hunks(&content, &hunks)?;
    crate::write(path, &result)?;
    modified.push(path.to_string_lossy().to_string());

    Ok(modified)
}

/// Parse a unified-diff string into hunks.  Ignores header lines.
fn parse_diff(input: &str) -> ToolResult<Vec<Hunk>> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<Hunk> = None;
    let lines_iter = input.lines().peekable();

    for line in lines_iter {
        if line.starts_with("@@") {
            // Parse hunk header: @@ -old_start,old_count +new_start,new_count @@
            let rest = line.trim_start_matches("@@").trim_end_matches("@@").trim();
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 {
                return Err(crate::ToolError::Generic(format!(
                    "malformed hunk header: {line}"
                )));
            }
            let old_part = parts[0].trim_start_matches('-');
            let new_part = parts[1].trim_start_matches('+');
            let (old_start, old_count) = parse_range(old_part);
            let (new_start, new_count) = parse_range(new_part);

            // Save previous hunk
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }

            current_hunk = Some(Hunk {
                old_start,
                old_count,
                new_start,
                new_count,
                lines: Vec::new(),
            });
        } else if line.starts_with("---") || line.starts_with("+++") || line.starts_with("diff ") {
            // Header lines — skip
            continue;
        } else if let Some(ref mut hunk) = current_hunk {
            // Accumulate hunk body
            if let Some(content) = line.strip_prefix('-') {
                hunk.lines.push(HunkLine::Removal(content.to_string()));
            } else if let Some(content) = line.strip_prefix('+') {
                hunk.lines.push(HunkLine::Addition(content.to_string()));
            } else if let Some(content) = line.strip_prefix(' ') {
                hunk.lines.push(HunkLine::Context(content.to_string()));
            } else if line.is_empty() {
                // Empty lines in a hunk are context (no leading space in some diffs)
                hunk.lines.push(HunkLine::Context(String::new()));
            }
            // Lines that don't match — skip (could be patch metadata)
        }
    }

    // Push the last hunk
    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }

    Ok(hunks)
}

fn parse_range(s: &str) -> (usize, usize) {
    if let Some((start, count)) = s.split_once(',') {
        (start.parse().unwrap_or(1), count.parse().unwrap_or(1))
    } else {
        (s.parse().unwrap_or(1), 1)
    }
}

/// Apply parsed hunks to the original content string.
fn apply_hunks(original: &str, hunks: &[Hunk]) -> ToolResult<String> {
    let lines: Vec<&str> = original.lines().collect();
    let mut result_lines: Vec<String> = Vec::new();
    let mut line_idx = 0usize; // 0-based index into `lines`

    for hunk in hunks {
        // The hunk old_start is 1-based; convert to 0-based
        let target_start = if hunk.old_start > 0 {
            hunk.old_start - 1
        } else {
            0
        };

        // Copy lines before the hunk
        while line_idx < target_start.min(lines.len()) {
            result_lines.push(lines[line_idx].to_string());
            line_idx += 1;
        }

        // Process hunk lines sequentially — additions go directly into result_lines
        // at the position they appear in the hunk (between context/removal lines).
        for hunk_line in &hunk.lines {
            match hunk_line {
                HunkLine::Context(expected) => {
                    let actual = if line_idx < lines.len() {
                        lines[line_idx]
                    } else {
                        ""
                    };
                    if actual != expected.as_str() {
                        return Err(crate::ToolError::Generic(format!(
                            "patch context mismatch at line {}: expected {expected:?}, got {actual:?}",
                            line_idx + 1,
                        )));
                    }
                    result_lines.push(actual.to_string());
                    line_idx += 1;
                }
                HunkLine::Removal(expected) => {
                    let actual = if line_idx < lines.len() {
                        lines[line_idx]
                    } else {
                        ""
                    };
                    if actual != expected.as_str() {
                        return Err(crate::ToolError::Generic(format!(
                            "patch removal mismatch at line {}: expected to remove {expected:?}, got {actual:?}",
                            line_idx + 1,
                        )));
                    }
                    line_idx += 1;
                }
                HunkLine::Addition(addition) => {
                    result_lines.push(addition.clone());
                }
            }
        }
    }

    // Copy remaining lines
    while line_idx < lines.len() {
        result_lines.push(lines[line_idx].to_string());
        line_idx += 1;
    }

    Ok(result_lines.join("\n") + if original.ends_with('\n') { "\n" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_apply_simple_add() {
        let patch = "\
--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
+    extra();
 }
";
        let input = "fn main() {\n    println!(\"old\");\n}\n";
        let expected = "fn main() {\n    println!(\"new\");\n    extra();\n}\n";
        let hunks = parse_diff(patch).unwrap();
        assert_eq!(hunks.len(), 1);
        let result = apply_hunks(input, &hunks).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_apply_remove_line() {
        let patch = "\
--- a/test.rs
+++ b/test.rs
@@ -1,4 +1,3 @@
 line1
 line2
-line3
 line4
";
        let input = "line1\nline2\nline3\nline4\n";
        let expected = "line1\nline2\nline4\n";
        let hunks = parse_diff(patch).unwrap();
        let result = apply_hunks(input, &hunks).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_apply_context_mismatch_fails() {
        let patch = "\
--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,3 @@
-aaa
+bbb
 ccc
";
        let input = "xxx\nccc\n";
        let hunks = parse_diff(patch).unwrap();
        let result = apply_hunks(input, &hunks);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("mismatch"));
    }

    #[test]
    fn test_apply_empty_patch_fails() {
        let result = parse_diff("");
        assert!(result.is_err() || result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_diff_multiple_hunks() {
        let patch = "\
--- a/test.rs
+++ b/test.rs
@@ -1,2 +1,2 @@
-old1
+new1
@@ -5,2 +5,2 @@
-old5
+new5
";
        let hunks = parse_diff(patch).unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[1].old_start, 5);
    }

    #[test]
    fn test_apply_patch_to_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.rs");
        fs::write(&file_path, "fn main() {\n    old_fn();\n}\n").unwrap();

        let patch = "\
--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,3 @@
 fn main() {
-    old_fn();
+    new_fn();
 }
"
        .to_string();
        let result = apply_patch(&file_path, &patch).unwrap();
        assert_eq!(result.len(), 1);

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "fn main() {\n    new_fn();\n}\n");
    }

    #[test]
    fn test_no_trailing_newline_input() {
        let patch = "\
--- a/test.rs
+++ b/test.rs
@@ -1,2 +1,2 @@
-old
+new
";
        let input = "old";
        let expected = "new";
        let hunks = parse_diff(patch).unwrap();
        let result = apply_hunks(input, &hunks).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_apply_add_only() {
        let patch = "\
--- a/test.rs
+++ b/test.rs
@@ -1,1 +1,2 @@
 line1
+line2
";
        let input = "line1\n";
        let expected = "line1\nline2\n";
        let hunks = parse_diff(patch).unwrap();
        let result = apply_hunks(input, &hunks).unwrap();
        assert_eq!(result, expected);
    }
}
