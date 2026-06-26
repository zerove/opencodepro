use glob::glob;
use anyhow::{Result, anyhow};

#[allow(dead_code)]
pub fn expand_glob(pattern: &str) -> Result<Vec<String>> {
    let paths = glob(pattern)
        .map_err(|e| anyhow!("Invalid glob pattern: {e}"))?;

    let mut results = Vec::new();
    for entry in paths {
        let path = entry.map_err(|e| anyhow!("Error reading glob entry: {e}"))?;
        results.push(path.to_string_lossy().to_string());
    }

    Ok(results)
}

#[allow(dead_code)]
pub fn resolve_alias(_name: &str) -> Option<String> {
    // In a real implementation, this would load aliases from a config file
    // or shell profile (.bashrc, .zshrc, etc.)
    // For now, we return None as no alias source is specified.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_expand_glob() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path();
        
        let file1 = dir_path.join("test1.txt");
        let file2 = dir_path.join("test2.txt");
        let file3 = dir_path.join("other.log");
        
        fs::File::create(&file1).unwrap();
        fs::File::create(&file2).unwrap();
        fs::File::create(&file3).unwrap();
        
        let pattern = format!("{}/*.txt", dir_path.to_string_lossy());
        let matches = expand_glob(&pattern).unwrap();
        
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&file1.to_string_lossy().to_string()));
        assert!(matches.contains(&file2.to_string_lossy().to_string()));
        assert!(!matches.contains(&file3.to_string_lossy().to_string()));
    }

    #[test]
    fn test_resolve_alias() {
        assert_eq!(resolve_alias("ll"), None);
    }
}
