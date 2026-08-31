//! Skill loading, parsing, rendering, and management.
//!
//! Skills are user-defined instruction sets loaded from SKILL.md files
//! with YAML frontmatter. They can inject context, block tools, or
//! customize agent behaviour.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (Skill, SkillSource, TriggerDef, SkillMeta).

pub mod types;

pub use types::{Skill, SkillSource, TriggerDef};

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ── 目录扫描 ──────────────────────────────────────────────────

/// 技能加载器，管理多级目录扫描与解析
#[derive(Debug)]
pub struct SkillLoader {
    dirs: Vec<PathBuf>,
}

impl SkillLoader {
    /// 创建一个空的加载器
    pub fn new() -> Self {
        SkillLoader { dirs: Vec::new() }
    }

    /// 添加用户全局技能目录 ~/.config/sisyphus/skills/
    pub fn with_user_dir(mut self) -> Self {
        if let Some(home) = dirs_next() {
            let p = home.join(".config").join("sisyphus").join("skills");
            self.dirs.push(p);
        }
        self
    }

    /// 添加项目本地技能目录
    pub fn with_project_dir(mut self, project_root: &Path) -> Self {
        let p = project_root.join(".sisyphus").join("skills");
        self.dirs.push(p);
        self
    }

    /// 添加自定义目录
    pub fn with_dir(mut self, dir: PathBuf) -> Self {
        self.dirs.push(dir);
        self
    }

    /// 添加内建技能目录
    pub fn with_builtin_dir(mut self, dir: PathBuf) -> Self {
        self.dirs.push(dir);
        self
    }

    /// 加载所有目录中的所有技能，按来源优先级合并（高优先级覆盖低优先级同名技能）
    pub fn load_all(&self) -> Vec<Skill> {
        let mut map: HashMap<String, Skill> = HashMap::new();

        for dir in &self.dirs {
            if !dir.exists() || !dir.is_dir() {
                continue;
            }
            let source = classify_source(dir);

            let entries = match fs::read_dir(dir) {
                Ok(e) => e,
                _ => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "md") {
                    if let Some(skill) = parse_skill_file(&path, source) {
                        let key = skill.name.clone();
                        let should_insert = match map.get(&key) {
                            Some(existing) => source.priority() > existing.source.priority(),
                            None => true,
                        };
                        if should_insert {
                            map.insert(key, skill);
                        }
                    }
                }
            }
        }

        let mut skills: Vec<Skill> = map.into_values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills
    }

    /// 查找匹配指定工具名称的技能
    pub fn find_by_tool<'a>(&self, tool_name: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
        skills
            .iter()
            .filter(|s| {
                s.triggers.is_empty()
                    || s.triggers
                        .iter()
                        .any(|t| t.tools.is_empty() || t.tools.iter().any(|tt| tt == tool_name))
            })
            .collect()
    }
}

impl Default for SkillLoader {
    fn default() -> Self {
        Self::new()
    }
}

// ── 来源分类 ──────────────────────────────────────────────────

fn classify_source(dir: &Path) -> SkillSource {
    let path_str = dir.to_string_lossy();
    if path_str.contains(".config/sisyphus/skills") {
        SkillSource::User
    } else if path_str.contains(".sisyphus/skills") {
        SkillSource::Project
    } else {
        SkillSource::BuiltIn
    }
}

// ── 文件解析 ──────────────────────────────────────────────────

/// 解析一个 .md 技能文件
fn parse_skill_file(path: &Path, source: SkillSource) -> Option<Skill> {
    let content = fs::read_to_string(path).ok()?;
    let (meta, body) = parse_frontmatter(&content);

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");

    Some(Skill {
        name: meta.name.unwrap_or_else(|| stem.to_string()),
        description: meta.description.unwrap_or_default(),
        version: meta.version,
        author: meta.author,
        tags: meta.tags,
        triggers: meta.triggers,
        instructions: body,
        source,
        source_path: Some(path.to_path_buf()),
    })
}

/// 从 markdown 中提取 YAML frontmatter（`---` 分隔）
fn parse_frontmatter(content: &str) -> (types::SkillMeta, String) {
    let content = content.trim();
    if !content.starts_with("---") {
        return (types::SkillMeta::default(), content.to_string());
    }

    let end = content[3..].find("---").map(|pos| pos + 3);
    match end {
        Some(end_pos) => {
            let yaml_str = &content[3..end_pos].trim();
            let body = content[end_pos + 3..].trim().to_string();

            match serde_yaml::from_str::<types::SkillMeta>(yaml_str) {
                Ok(meta) => (meta, body),
                Err(_) => (types::SkillMeta::default(), content.to_string()),
            }
        },
        None => (types::SkillMeta::default(), content.to_string()),
    }
}

// ── 呈现 ──────────────────────────────────────────────────────

impl std::fmt::Display for Skill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} v{}", self.name, self.version)?;
        if !self.description.is_empty() {
            writeln!(f, "  {}", self.description)?;
        }
        if let Some(ref author) = self.author {
            writeln!(f, "  author: {author}")?;
        }
        if !self.tags.is_empty() {
            writeln!(f, "  tags: {}", self.tags.join(", "))?;
        }
        if !self.triggers.is_empty() {
            writeln!(f, "  triggers:")?;
            for t in &self.triggers {
                let desc = t.description.as_deref().unwrap_or(t.pattern.as_deref().unwrap_or("*"));
                writeln!(f, "    - {desc}")?;
                if !t.tools.is_empty() {
                    writeln!(f, "      tools: {}", t.tools.join(", "))?;
                }
            }
        }
        writeln!(f, "  source: {:?}", self.source)?;
        Ok(())
    }
}

/// Render a skill's instructions with optional variable substitution
pub fn render_instructions(skill: &Skill, vars: &HashMap<String, String>) -> String {
    let mut result = skill.instructions.clone();
    for (key, val) in vars {
        result = result.replace(&format!("{{{{{key}}}}}"), val);
    }
    result
}

/// Copy a SKILL.md file into the user's skill directory, returning the skill name.
pub fn install_skill(source: &Path) -> Result<String, String> {
    let home = dirs_next().ok_or("Cannot determine home directory")?;
    let dest_dir = home.join(".config").join("sisyphus").join("skills");
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("Failed to create skill dir: {e}"))?;

    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid source filename".to_string())?;
    let dest = dest_dir.join(format!("{stem}.md"));

    // Parse to validate and extract the skill name
    let content =
        std::fs::read_to_string(source).map_err(|e| format!("Cannot read source: {e}"))?;
    let (meta, _body) = parse_frontmatter(&content);
    let name = meta.name.unwrap_or_else(|| stem.to_string());

    std::fs::copy(source, &dest).map_err(|e| format!("Failed to copy skill: {e}"))?;
    Ok(name)
}

/// Remove an installed skill by name from the user's skill directory.
pub fn remove_skill(name: &str) -> Result<(), String> {
    let home = dirs_next().ok_or("Cannot determine home directory")?;
    let dir = home.join(".config").join("sisyphus").join("skills");
    let path = dir.join(format!("{name}.md"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Failed to remove skill: {e}"))
    } else {
        Err(format!("Skill '{name}' not found"))
    }
}

// ── Helpers ───────────────────────────────────────────────────

fn dirs_next() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
#[path = "skill_tests.rs"]
mod tests;
