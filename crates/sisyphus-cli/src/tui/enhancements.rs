//! TUI 增强：斜杠命令补全、@file 引用、模糊输入搜索。

use std::path::Path;

/// 补全弹窗的斜杠命令定义。
#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
}

/// 所有可用的斜杠命令。
pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/help",
        description: "Show keybindings & help",
        category: "General",
    },
    SlashCommand {
        name: "/exit",
        description: "Exit Sisyphus",
        category: "General",
    },
    SlashCommand {
        name: "/clear",
        description: "Clear the log",
        category: "General",
    },
    SlashCommand {
        name: "/theme",
        description: "Toggle dark/light theme",
        category: "General",
    },
    SlashCommand {
        name: "/sessions",
        description: "Switch or manage sessions",
        category: "Session",
    },
    SlashCommand {
        name: "/new",
        description: "Create a new session",
        category: "Session",
    },
    SlashCommand {
        name: "/model",
        description: "Switch AI model",
        category: "Agent",
    },
    SlashCommand {
        name: "/agent",
        description: "Switch agent",
        category: "Agent",
    },
    SlashCommand {
        name: "/dev",
        description: "Run autonomous dev agent",
        category: "Agent",
    },
    SlashCommand {
        name: "/todos",
        description: "Manage todo list",
        category: "Tools",
    },
    SlashCommand {
        name: "/lsp",
        description: "View LSP diagnostics",
        category: "Tools",
    },
    SlashCommand {
        name: "/sop",
        description: "List & execute SOPs",
        category: "Tools",
    },
    SlashCommand {
        name: "/skills",
        description: "Browse & install skills",
        category: "Tools",
    },
    SlashCommand {
        name: "/roadmap",
        description: "View project roadmap",
        category: "Tools",
    },
    SlashCommand {
        name: "/bridge",
        description: "Check bridge health",
        category: "Tools",
    },
    SlashCommand {
        name: "/home",
        description: "Return to Home screen",
        category: "Navigation",
    },
    SlashCommand {
        name: "/search",
        description: "Search messages in session",
        category: "Session",
    },
];

/// 查询对目标字符串的模糊匹配得分。
/// 返回 (score, matched_indices)，得分越高越好。
/// 查询不匹配时返回 None。
pub fn fuzzy_match(query: &str, target: &str) -> Option<(i64, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, vec![]));
    }

    let query_lower: Vec<char> = query.to_lowercase().chars().collect();
    let target_lower: Vec<char> = target.to_lowercase().chars().collect();

    let mut query_idx = 0;
    let mut matched_indices = Vec::new();
    let mut score: i64 = 0;
    let mut last_match_idx: Option<usize> = None;

    for (i, &tc) in target_lower.iter().enumerate() {
        if query_idx < query_lower.len() && tc == query_lower[query_idx] {
            matched_indices.push(i);

            // 词首匹配加分
            if i == 0 || target_lower[i - 1] == ' ' || target_lower[i - 1] == '/' {
                score += 10;
            }
            // 连续匹配加分
            if let Some(last) = last_match_idx {
                if i == last + 1 {
                    score += 5;
                }
            }
            // 大小写完全匹配加分
            if target.chars().nth(i) == Some(query_lower[query_idx]) {
                score += 1;
            }

            last_match_idx = Some(i);
            query_idx += 1;
        }
    }

    if query_idx == query_lower.len() {
        // 所有查询字符都匹配
        // 较长目标扣分（偏好更短/更具体的匹配）
        score -= (target.len() as i64) / 10;
        Some((score, matched_indices))
    } else {
        None
    }
}

/// 通过模糊匹配过滤并排序斜杠命令。
pub fn filter_slash_commands(query: &str) -> Vec<(&'static SlashCommand, i64, Vec<usize>)> {
    let q = query.strip_prefix('/').unwrap_or(query);

    let mut results: Vec<_> = SLASH_COMMANDS
        .iter()
        .filter_map(|cmd| {
            let name_no_slash = cmd.name.strip_prefix('/').unwrap_or(cmd.name);
            if let Some((score, indices)) = fuzzy_match(q, name_no_slash) {
                // 调整索引以计入命令名开头的 '/'
                let adjusted: Vec<usize> = indices.into_iter().map(|i| i + 1).collect();
                Some((cmd, score + 5, adjusted)) // +5 for name match
            } else if let Some((score, indices)) = fuzzy_match(q, cmd.description) {
                Some((cmd, score, indices))
            } else {
                None
            }
        })
        .collect();

    results.sort_by_key(|b| std::cmp::Reverse(b.1));
    results
}

/// 用户输入中已解析的 @file 引用。
#[derive(Debug, Clone)]
pub struct FileReference {
    pub start: usize,
    pub end: usize,
    pub path: String,
}

/// 从用户输入解析 @file 引用。
/// 查找类似 `@path/to/file.rs` 或 `@file.txt` 的模式。
pub fn parse_file_references(input: &str) -> Vec<FileReference> {
    let mut refs = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace() || chars[i - 1] == '"') {
            let start = i;
            i += 1; // skip '@'
            let mut path = String::new();

            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '"' && chars[i] != ')'
            {
                path.push(chars[i]);
                i += 1;
            }

            if !path.is_empty() {
                refs.push(FileReference {
                    start,
                    end: i,
                    path,
                });
            }
        } else {
            i += 1;
        }
    }

    refs
}

/// 将文件引用解析为其内容，截断到 max_lines。
pub fn resolve_file_reference(
    file_ref: &FileReference,
    workspace_root: &Path,
    max_lines: usize,
) -> Result<String, String> {
    let path = workspace_root.join(&file_ref.path);

    if !path.exists() {
        return Err(format!("File not found: {}", file_ref.path));
    }

    if !path.is_file() {
        return Err(format!("Not a file: {}", file_ref.path));
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {e}", file_ref.path))?;

    let lines: Vec<&str> = content.lines().take(max_lines).collect();
    let truncated = lines.len() < content.lines().count();

    let mut result = lines.join("\n");
    if truncated {
        result.push_str(&format!(
            "\n... ({} more lines)",
            content.lines().count() - max_lines
        ));
    }

    Ok(result)
}

/// 展开输入文本中的所有 @file 引用，替换为文件内容。
pub fn expand_file_references(input: &str, workspace_root: &Path) -> String {
    let refs = parse_file_references(input);
    if refs.is_empty() {
        return input.to_string();
    }

    let mut result = input.to_string();

    // 逆序处理以保持索引正确
    for file_ref in refs.iter().rev() {
        match resolve_file_reference(file_ref, workspace_root, 200) {
            Ok(content) => {
                let replacement = format!("[File: {}]\n```\n{}\n```", file_ref.path, content);
                result.replace_range(file_ref.start..file_ref.end, &replacement);
            }
            Err(e) => {
                let replacement = format!("[Error: {e}]");
                result.replace_range(file_ref.start..file_ref.end, &replacement);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_exact() {
        let (score, indices) = fuzzy_match("help", "help").unwrap();
        assert!(score > 0);
        assert_eq!(indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_fuzzy_match_partial() {
        let result = fuzzy_match("hlp", "help");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_no_match() {
        assert!(fuzzy_match("xyz", "help").is_none());
    }

    #[test]
    fn test_fuzzy_match_empty_query() {
        let (score, indices) = fuzzy_match("", "help").unwrap();
        assert_eq!(score, 0);
        assert!(indices.is_empty());
    }

    #[test]
    fn test_filter_slash_commands() {
        let results = filter_slash_commands("/he");
        assert!(!results.is_empty());
        assert_eq!(results[0].0.name, "/help");
    }

    #[test]
    fn test_parse_file_references() {
        let refs = parse_file_references("look at @src/main.rs and @lib.rs");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].path, "src/main.rs");
        assert_eq!(refs[1].path, "lib.rs");
    }

    #[test]
    fn test_parse_no_references() {
        let refs = parse_file_references("no files here");
        assert!(refs.is_empty());
    }

    #[test]
    fn test_expand_file_references_no_refs() {
        let input = "no files here";
        assert_eq!(expand_file_references(input, Path::new(".")), input);
    }
}
