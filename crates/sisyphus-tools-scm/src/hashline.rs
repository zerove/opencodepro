use sisyphus_tools_core::{ToolError, ToolResult};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashlineData {
    pub line_number: usize,
    pub content_hash: String,
    pub content_snippet: String,
}

/// Generates a SHA-like hash of the content using DefaultHasher.
pub fn hash_content(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let hex = format!("{:016x}", hasher.finish());
    hex[..8].to_string()
}

/// Determines the comment syntax based on the file extension.
fn get_comment_syntax(path: &Path) -> (&'static str, Option<&'static str>) {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    match ext {
        "rs" | "js" | "ts" | "go" => ("//", None),
        "py" | "rb" | "sh" | "yaml" | "toml" => ("#", None),
        "sql" | "hs" => ("--", None),
        "html" | "xml" => ("<!--", Some("-->")),
        _ => ("//", None), // Default to //
    }
}

/// Generates a LINE#ID comment: e.g., "// LINE#42:a1b2c3d4"
pub fn generate_hashline(path: &Path, line_number: usize, content: &str) -> String {
    let hash = hash_content(content);
    let (start, end) = get_comment_syntax(path);

    if let Some(end_tag) = end {
        format!(" {start} LINE#{line_number}:{hash} {end_tag}")
    } else {
        format!(" {start} LINE#{line_number}:{hash}")
    }
}

/// Parses a LINE#ID comment back into components.
pub fn parse_hashline(line: &str) -> Option<HashlineData> {
    // Look for "LINE#<num>:<hash>"
    let marker = "LINE#";
    if let Some(start_idx) = line.find(marker) {
        let remaining = &line[start_idx + marker.len()..];
        let parts: Vec<&str> = remaining.split(':').collect();
        if parts.len() >= 2 {
            let line_num = parts[0].parse::<usize>().ok()?;
            let hash = parts[1].split_whitespace().next()?.to_string();

            // We don't have the snippet here, so we'll leave it empty or fill it later
            return Some(HashlineData {
                line_number: line_num,
                content_hash: hash,
                content_snippet: String::new(),
            });
        }
    }
    None
}

/// Verifies the actual line at the given number against the stored hash.
pub fn verify_line(base_content: &str, hashline: &str) -> Result<bool, String> {
    let data = parse_hashline(hashline).ok_or_else(|| "Invalid hashline format".to_string())?;

    let lines: Vec<&str> = base_content.lines().collect();
    if data.line_number == 0 || data.line_number > lines.len() {
        return Err(format!(
            "Line number {} out of bounds (file has {} lines)",
            data.line_number,
            lines.len()
        ));
    }

    let actual_line = lines[data.line_number - 1];
    let content_without_hashline = strip_hashline(actual_line);

    let actual_hash = hash_content(content_without_hashline);
    Ok(actual_hash == data.content_hash)
}

fn strip_hashline(line: &str) -> &str {
    if let Some(idx) = line.rfind(" LINE#") {
        // The hashline starts with a space, then the marker, then " LINE#".
        // We need to find the space that starts the whole comment.
        // Since the marker is variable, we look for the space before the marker.
        let search_area = &line[..idx];
        if let Some(space_idx) = search_area.rfind(' ') {
            return &line[..space_idx];
        }
    }
    line
}

/// Adds LINE#ID comments to all non-empty lines in a file.
pub fn annotate_file(file_path: &Path) -> ToolResult<usize> {
    let content = fs::read_to_string(file_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut annotated_lines = Vec::new();
    let mut count = 0;

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        if line.trim().is_empty() {
            annotated_lines.push(line.to_string());
            continue;
        }

        let hashline = generate_hashline(file_path, line_num, line);
        annotated_lines.push(format!("{line}{hashline}"));
        count += 1;
    }

    let new_content = annotated_lines.join("\n");
    // Use atomic write pattern from file.rs
    let parent = file_path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp_file = NamedTempFile::new_in(parent).map_err(ToolError::Io)?;
    temp_file
        .write_all(new_content.as_bytes())
        .map_err(ToolError::Io)?;
    temp_file
        .persist(file_path)
        .map_err(|e| ToolError::Io(e.error))?;

    Ok(count)
}

/// Finds a line by LINE#ID, verifies hash, replaces content, returns old content on success.
pub fn edit_with_hashline(
    file_path: &Path,
    target_hashline: &str,
    new_content: &str,
) -> ToolResult<String> {
    let content = fs::read_to_string(file_path)?;
    let data = parse_hashline(target_hashline)
        .ok_or_else(|| ToolError::Generic("Invalid hashline format".to_string()))?;

    let lines: Vec<String> = content
        .lines()
        .map(std::string::ToString::to_string)
        .collect();
    if data.line_number == 0 || data.line_number > lines.len() {
        return Err(ToolError::Generic(format!(
            "Line number {} out of bounds",
            data.line_number
        )));
    }

    let actual_line = &lines[data.line_number - 1];
    let content_without_hashline = strip_hashline(actual_line);

    let actual_hash = hash_content(content_without_hashline);
    if actual_hash != data.content_hash {
        return Err(ToolError::Generic(format!(
            "Hash mismatch! Expected {}, found {}. Line content: '{}'",
            data.content_hash, actual_hash, content_without_hashline
        )));
    }

    let old_content = content_without_hashline.to_string();

    // Construct the new line with the SAME hashline (to keep it trackable)
    let (start, end) = get_comment_syntax(file_path);
    let hashline_comment = if let Some(end_tag) = end {
        format!(
            " {} LINE#{}:{} {}",
            start, data.line_number, data.content_hash, end_tag
        )
    } else {
        format!(" {} LINE#{}:{}", start, data.line_number, data.content_hash)
    };

    let mut new_lines = lines.clone();
    new_lines[data.line_number - 1] = format!("{new_content}{hashline_comment}");

    let final_content = new_lines.join("\n");
    let parent = file_path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp_file = NamedTempFile::new_in(parent).map_err(ToolError::Io)?;
    temp_file
        .write_all(final_content.as_bytes())
        .map_err(ToolError::Io)?;
    temp_file
        .persist(file_path)
        .map_err(|e| ToolError::Io(e.error))?;

    Ok(old_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_content() {
        let h1 = hash_content("hello world");
        let h2 = hash_content("hello world");
        let h3 = hash_content("hello worle");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 8);
    }

    #[test]
    fn test_generate_hashline_syntax() {
        let rs_path = Path::new("test.rs");
        let py_path = Path::new("test.py");
        let sql_path = Path::new("test.sql");
        let html_path = Path::new("test.html");

        let content = "fn main() {}";
        let h_rs = generate_hashline(rs_path, 1, content);
        let h_py = generate_hashline(py_path, 1, content);
        let h_sql = generate_hashline(sql_path, 1, content);
        let h_html = generate_hashline(html_path, 1, content);

        assert!(h_rs.contains("// LINE#1:"));
        assert!(h_py.contains("# LINE#1:"));
        assert!(h_sql.contains("-- LINE#1:"));
        assert!(h_html.contains("<!-- LINE#1:") && h_html.contains("-->"));
    }

    #[test]
    fn test_parse_hashline() {
        let line = "some code // LINE#42:a1b2c3d4";
        let data = parse_hashline(line).unwrap();
        assert_eq!(data.line_number, 42);
        assert_eq!(data.content_hash, "a1b2c3d4");
    }

    #[test]
    fn test_verify_line() {
        let content = "line1\nline2\nline3";
        let hash = hash_content("line2");
        let hashline = format!("// LINE#2:{}", hash);

        assert!(verify_line(content, &hashline).unwrap());

        let wrong_hashline = "// LINE#2:wronghash".to_string();
        assert!(!verify_line(content, &wrong_hashline).unwrap());

        let out_of_bounds = format!("// LINE#10:{}", hash);
        assert!(verify_line(content, &out_of_bounds).is_err());
    }

    #[test]
    fn test_annotate_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.rs");
        fs::write(&path, "line1\n\nline3").unwrap();

        let count = annotate_file(&path).unwrap();
        assert_eq!(count, 2);

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines[0].contains("LINE#1:"));
        assert_eq!(lines[1], "");
        assert!(lines[2].contains("LINE#3:"));
    }

    #[test]
    fn test_edit_with_hashline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.rs");
        let original_content = "line1\nline2\nline3";
        fs::write(&path, original_content).unwrap();

        let hash = hash_content("line2");
        let _hashline = format!("// LINE#2:{}", hash);

        // First, we must annotate the file so it has the hashline we're targeting
        let actual_hash = hash_content("line2");
        let annotated = format!("line1\nline2 // LINE#2:{}\nline3", actual_hash);
        fs::write(&path, annotated).unwrap();

        let target = format!("// LINE#2:{}", actual_hash);
        let old = edit_with_hashline(&path, &target, "new_line2").unwrap();
        assert_eq!(old, "line2");

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("new_line2 // LINE#2:"));

        // Test conflict
        let conflict_hashline = "// LINE#2:wronghash".to_string();
        let result = edit_with_hashline(&path, &conflict_hashline, "fail");
        assert!(result.is_err());
    }
}
