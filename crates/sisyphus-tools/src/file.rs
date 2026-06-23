use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use crate::{ToolError, ToolResult};
use regex::Regex;
use ignore::WalkBuilder;
use tempfile::NamedTempFile;

#[derive(Debug)]
pub struct GrepMatch {
    pub file: PathBuf,
    pub line: usize,
    pub content: String,
}

pub fn read(path: &Path) -> ToolResult<String> {
    fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::NotFound(path.to_string_lossy().into_owned())
        } else {
            ToolError::Io(e)
        }
    })
}

pub fn read_lines(path: &Path, offset: usize, limit: usize) -> ToolResult<String> {
    let file = fs::File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::NotFound(path.to_string_lossy().into_owned())
        } else {
            ToolError::Io(e)
        }
    })?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    for _ in 0..offset {
        lines.next();
    }

    let mut result = Vec::new();
    for line in lines.take(limit) {
        let line = line?;
        result.push(line);
    }

    Ok(result.join("\n"))
}

/// Atomic write: writes to a temporary file and then renames it to the target path.
/// This ensures that the target file is never left in a partially written state.
pub fn write(path: &Path, content: &str) -> ToolResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Create a temp file in the same directory as the target to ensure they are on the same mount point
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp_file = NamedTempFile::new_in(parent).map_err(ToolError::Io)?;
    
    temp_file.write_all(content.as_bytes()).map_err(ToolError::Io)?;
    temp_file.persist(path).map_err(|e| ToolError::Io(e.error))?;
    
    Ok(())
}

pub fn edit(path: &Path, old_string: &str, new_string: &str) -> ToolResult<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp_file = NamedTempFile::new_in(parent).map_err(ToolError::Io)?;
    
    let file = fs::File::open(path).map_err(ToolError::Io)?;
    let mut reader = BufReader::new(file);
    let mut replaced = false;
    let mut line_buffer = String::new();
    
    loop {
        line_buffer.clear();
        let bytes_read = reader.read_line(&mut line_buffer).map_err(ToolError::Io)?;
        if bytes_read == 0 {
            break; // EOF
        }
        
        if !replaced && line_buffer.contains(old_string) {
            if line_buffer.matches(old_string).count() > 1 {
                return Err(ToolError::EditAmbiguous);
            }
            let new_line = line_buffer.replace(old_string, new_string);
            temp_file.write_all(new_line.as_bytes()).map_err(ToolError::Io)?;
            replaced = true;
        } else {
            temp_file.write_all(line_buffer.as_bytes()).map_err(ToolError::Io)?;
        }
    }
    
    if !replaced {
        return Err(ToolError::EditNoMatch);
    }
    
    temp_file.persist(path).map_err(|e| ToolError::Io(e.error))?;
    Ok(())
}

pub fn edit_all(path: &Path, old_string: &str, new_string: &str) -> ToolResult<usize> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp_file = NamedTempFile::new_in(parent).map_err(ToolError::Io)?;
    
    let file = fs::File::open(path).map_err(ToolError::Io)?;
    let mut reader = BufReader::new(file);
    let mut count = 0;
    let mut line_buffer = String::new();
    
    loop {
        line_buffer.clear();
        let bytes_read = reader.read_line(&mut line_buffer).map_err(ToolError::Io)?;
        if bytes_read == 0 {
            break; // EOF
        }
        
        if line_buffer.contains(old_string) {
            count += line_buffer.matches(old_string).count();
            let new_line = line_buffer.replace(old_string, new_string);
            temp_file.write_all(new_line.as_bytes()).map_err(ToolError::Io)?;
        } else {
            temp_file.write_all(line_buffer.as_bytes()).map_err(ToolError::Io)?;
        }
    }
    
    if count == 0 {
        return Err(ToolError::EditNoMatch);
    }
    
    temp_file.persist(path).map_err(|e| ToolError::Io(e.error))?;
    Ok(count)
}

pub fn glob(pattern: &str, root: &Path) -> ToolResult<Vec<PathBuf>> {
    let full_pattern = if root == Path::new("/") {
        pattern.to_string()
    } else {
        root.join(pattern).to_string_lossy().into_owned()
    };

    let entries = ::glob::glob(&full_pattern).map_err(|e| ToolError::GlobPattern(e.to_string()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .collect();

    paths.sort_by(|a, b| {
        let a_time = fs::metadata(a).and_then(|m| m.modified()).ok();
        let b_time = fs::metadata(b).and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    if paths.len() > 100 {
        paths.truncate(100);
    }

    Ok(paths)
}

pub fn grep(pattern: &str, path: &Path, include: Option<&str>) -> ToolResult<Vec<GrepMatch>> {
    let re = Regex::new(pattern).map_err(|e| ToolError::GrepError(e.to_string()))?;
    let mut matches = Vec::new();

    if path.is_dir() {
        let walker = WalkBuilder::new(path).build();
        for entry in walker {
            let entry = entry.map_err(|e| ToolError::GrepError(e.to_string()))?;
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                let file_path = entry.path();
                if let Some(inc) = include {
                    if !file_path.to_string_lossy().contains(inc) {
                        continue;
                    }
                }
                matches.extend(grep_file(&re, file_path)?);
            }
        }
    } else {
        matches.extend(grep_file(&re, path)?);
    }

    Ok(matches)
}

fn grep_file(re: &Regex, path: &Path) -> ToolResult<Vec<GrepMatch>> {
    let file = fs::File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            ToolError::PermissionDenied(path.to_string_lossy().into_owned())
        } else {
            ToolError::Io(e)
        }
    })?;

    let reader = BufReader::new(file);
    let mut file_matches = Vec::new();
    
    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        if re.is_match(&line) {
            file_matches.push(GrepMatch {
                file: path.to_path_buf(),
                line: i + 1,
                content: line,
            });
        }
    }
    Ok(file_matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(content: &str) -> (tempfile::NamedTempFile, std::path::PathBuf) {
        let mut f = tempfile::NamedTempFile::new().expect("create temp file");
        write!(f, "{}", content).expect("write temp content");
        let path = f.path().to_path_buf();
        (f, path)
    }

    #[test]
    fn test_read_file() {
        let (_f, path) = temp_file("hello\nworld");
        let result = read(&path).unwrap();
        assert_eq!(result, "hello\nworld");
    }

    #[test]
    fn test_read_not_found() {
        let result = read(std::path::Path::new("/tmp/__nonexistent_098765__"));
        assert!(matches!(result.unwrap_err(), ToolError::NotFound(_)));
    }

    #[test]
    fn test_write_and_read() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("test_write.txt");
        write(&path, "new content").unwrap();
        let content = read(&path).unwrap();
        assert_eq!(content, "new content");
    }

    #[test]
    fn test_write_creates_parent_dir() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("sub/deep/file.txt");
        write(&path, "nested").unwrap();
        assert!(path.exists());
        assert_eq!(read(&path).unwrap(), "nested");
    }

    #[test]
    fn test_read_lines_offset() {
        let (_f, path) = temp_file("line1\nline2\nline3\nline4");
        let result = read_lines(&path, 1, 2).unwrap();
        assert_eq!(result, "line2\nline3");
    }

    #[test]
    fn test_read_lines_offset_beyond_end() {
        let (_f, path) = temp_file("a\nb");
        let result = read_lines(&path, 10, 5).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_edit_single_match() {
        let (_f, path) = temp_file("foo bar baz");
        edit(&path, "bar", "QUX").unwrap();
        assert_eq!(read(&path).unwrap(), "foo QUX baz");
    }

    #[test]
    fn test_edit_all_no_match() {
        let (_f, path) = temp_file("hello");
        let result = edit_all(&path, "nope", "x");
        assert!(matches!(result.unwrap_err(), ToolError::EditNoMatch));
    }

    #[test]
    fn test_edit_all_empty_file() {
        let (_f, path) = temp_file("");
        let result = edit_all(&path, "any", "thing");
        assert!(matches!(result.unwrap_err(), ToolError::EditNoMatch));
    }

    #[test]
    fn test_glob_no_match() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let results = glob("*.nonexistent", dir.path()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_grep_empty_file() {
        let (_f, path) = temp_file("");
        let results = grep("pattern", &path, None).unwrap();
        assert!(results.is_empty());
    }


    #[test]
    fn test_edit_ambiguous() {
        let (_f, path) = temp_file("ab ab ab");
        let result = edit(&path, "ab", "xx");
        assert!(matches!(result.unwrap_err(), ToolError::EditAmbiguous));
    }

    #[test]
    fn test_edit_all() {
        let (_f, path) = temp_file("ab ab ab");
        let count = edit_all(&path, "ab", "xx").unwrap();
        assert_eq!(count, 3);
        assert_eq!(read(&path).unwrap(), "xx xx xx");
    }

    #[test]
    fn test_read_empty_file() {
        let (_f, path) = temp_file("");
        assert_eq!(read(&path).unwrap(), "");
    }

    #[test]
    fn test_read_directory_as_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let result = read(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_write_empty_content() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("empty.txt");
        write(&path, "").unwrap();
        assert_eq!(read(&path).unwrap(), "");
    }

    #[test]
    fn test_glob_root() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f1 = dir.path().join("alpha.txt");
        let f2 = dir.path().join("beta.rs");
        std::fs::write(&f1, "").unwrap();
        std::fs::write(&f2, "").unwrap();

        let results = glob("*.txt", dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name().unwrap(), "alpha.txt");
    }

    #[test]
    fn test_glob_limit() {
        let dir = tempfile::tempdir().expect("create temp dir");
        for i in 0..150 {
            let f = dir.path().join(format!("file_{}.txt", i));
            std::fs::write(&f, "").unwrap();
        }
        let results = glob("*.txt", dir.path()).unwrap();
        assert!(results.len() <= 100, "glob should be truncated to 100");
    }

    #[test]
    fn test_grep_file() {
        let (_f, path) = temp_file("apple\nbanana\ncherry\nApple");
        let results = grep("(?i)apple", &path, None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].line, 1);
        assert_eq!(results[1].line, 4);
    }

    #[test]
    fn test_grep_no_match() {
        let (_f, path) = temp_file("one\ntwo\nthree");
        let results = grep("xyz", &path, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_grep_invalid_regex() {
        let (_f, path) = temp_file("foo");
        let result = grep("[invalid", &path, None);
        assert!(matches!(result.unwrap_err(), ToolError::GrepError(_)));
    }
}
