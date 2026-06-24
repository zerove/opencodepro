use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// 一个可加载的技能定义，对应一个 SKILL.md 文件。
///
/// 技能系统让用户可以定义自定义指令集，在工具执行时注入上下文。
/// 格式参考 opencode SKILL.md：YAML frontmatter + Markdown 正文。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// 技能唯一名称（从文件名推导或 frontmatter 指定）
    pub name: String,
    /// 简短描述
    pub description: String,
    /// 版本号
    #[serde(default = "default_version")]
    pub version: String,
    /// 作者
    #[serde(default)]
    pub author: Option<String>,
    /// 标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 触发条件列表
    #[serde(default)]
    pub triggers: Vec<TriggerDef>,
    /// 完整指令正文（frontmatter 之后的内容）
    #[serde(skip)]
    pub instructions: String,
    /// 来源分类
    #[serde(skip)]
    pub source: SkillSource,
    /// 来源文件路径
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

fn default_version() -> String {
    "0.1.0".into()
}

/// 技能来源层级
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    /// 内建技能（发行版自带）
    #[default]
    BuiltIn,
    /// 用户全局技能 (~/.config/sisyphus/skills/)
    User,
    /// 项目本地技能 (.sisyphus/skills/)
    Project,
}

impl SkillSource {
    pub fn priority(&self) -> u8 {
        match self {
            SkillSource::Project => 30,
            SkillSource::User => 20,
            SkillSource::BuiltIn => 10,
        }
    }
}

/// 触发条件定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDef {
    /// 触发的关键字/正则模式
    pub pattern: Option<String>,
    /// 关联的工具名称列表（空表示全部）
    #[serde(default)]
    pub tools: Vec<String>,
    /// 触发描述
    pub description: Option<String>,
}

// ── YAML Frontmatter ──────────────────────────────────────────

/// SKILL.md 的 frontmatter 结构
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillMeta {
    name: Option<String>,
    description: Option<String>,
    #[serde(default = "default_version")]
    version: String,
    author: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    triggers: Vec<TriggerDef>,
}

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
                if path.extension().map(|e| e == "md").unwrap_or(false) {
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
                    || s.triggers.iter().any(|t| {
                        t.tools.is_empty() || t.tools.iter().any(|tt| tt == tool_name)
                    })
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
fn parse_frontmatter(content: &str) -> (SkillMeta, String) {
    let content = content.trim();
    if !content.starts_with("---") {
        return (SkillMeta::default(), content.to_string());
    }

    let end = content[3..].find("---").map(|pos| pos + 3);
    match end {
        Some(end_pos) => {
            let yaml_str = &content[3..end_pos].trim();
            let body = content[end_pos + 3..].trim().to_string();

            match serde_yaml::from_str::<SkillMeta>(yaml_str) {
                Ok(meta) => (meta, body),
                Err(_) => (SkillMeta::default(), content.to_string()),
            }
        }
        None => (SkillMeta::default(), content.to_string()),
    }
}

impl Default for SkillMeta {
    fn default() -> Self {
        SkillMeta {
            name: None,
            description: None,
            version: "0.1.0".into(),
            author: None,
            tags: Vec::new(),
            triggers: Vec::new(),
        }
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
                let desc = t
                    .description
                    .as_deref()
                    .unwrap_or(t.pattern.as_deref().unwrap_or("*"));
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
        result = result.replace(&format!("{{{{{}}}}}", key), val);
    }
    result
}

// ── Helpers ───────────────────────────────────────────────────

fn dirs_next() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("USERPROFILE").ok().map(PathBuf::from)
        })
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_empty() {
        let (meta, body) = parse_frontmatter("just some text");
        assert_eq!(meta.name, None);
        assert_eq!(body, "just some text");
    }

    #[test]
    fn test_parse_frontmatter_full() {
        let content = r#"---
name: my-test-skill
description: A test skill
version: "1.0.0"
author: test-user
tags: [rust, testing]
triggers:
  - tools: ["metis_analyse"]
    description: Activated during metis analysis
---

This is the instruction body.
"#;
        let (meta, body) = parse_frontmatter(content);
        assert_eq!(meta.name, Some("my-test-skill".into()));
        assert_eq!(meta.description, Some("A test skill".into()));
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.author, Some("test-user".into()));
        assert_eq!(meta.tags, vec!["rust", "testing"]);
        assert_eq!(meta.triggers.len(), 1);
        assert_eq!(meta.triggers[0].tools, vec!["metis_analyse"]);
        assert_eq!(meta.triggers[0].description, Some("Activated during metis analysis".into()));
        assert!(body.contains("instruction body"));
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "plain markdown body";
        let (meta, body) = parse_frontmatter(content);
        assert_eq!(meta.name, None);
        assert_eq!(body, "plain markdown body");
    }

    #[test]
    fn test_priority_ordering() {
        assert!(SkillSource::Project.priority() > SkillSource::User.priority());
        assert!(SkillSource::User.priority() > SkillSource::BuiltIn.priority());
    }

    #[test]
    fn test_render_instructions() {
        let skill = Skill {
            name: "test".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            triggers: vec![],
            instructions: "Hello {{name}}, your tool is {{tool}}".into(),
            source: SkillSource::BuiltIn,
            source_path: None,
        };
        let mut vars = HashMap::new();
        vars.insert("name".into(), "Sisyphus".into());
        vars.insert("tool".into(), "metis".into());
        let result = render_instructions(&skill, &vars);
        assert_eq!(result, "Hello Sisyphus, your tool is metis");
    }

    #[test]
    fn test_find_by_tool() {
        let skills = vec![
            Skill {
                name: "metis-helper".into(),
                description: "".into(),
                version: "0.1.0".into(),
                author: None,
                tags: vec![],
                triggers: vec![TriggerDef {
                    pattern: None,
                    tools: vec!["metis_analyse".into()],
                    description: None,
                }],
                instructions: "metis instructions".into(),
                source: SkillSource::BuiltIn,
                source_path: None,
            },
            Skill {
                name: "generic".into(),
                description: "".into(),
                version: "0.1.0".into(),
                author: None,
                tags: vec![],
                triggers: vec![],
                instructions: "generic".into(),
                source: SkillSource::BuiltIn,
                source_path: None,
            },
        ];
        let loader = SkillLoader::new();
        let matched = loader.find_by_tool("metis_analyse", &skills);
        assert_eq!(matched.len(), 2); // both match: one by trigger, one by empty triggers
    }
}
