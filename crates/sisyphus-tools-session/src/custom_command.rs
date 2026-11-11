//! 自定义开发命令（`.sisyphus/commands/*.md`）。
//!
//! 一个命令文件由 YAML frontmatter + 正文组成；正文就是模板，支持 `{{arg}}`
//! 占位符替换。命令有两种形态：
//!
//! - **提示词命令**（无 `run`）：渲染后作为提示词交给代理/人；
//! - **Shell 命令**（有 `run`）：`run` 是命令模板，渲染后可执行。
//!
//! ```markdown
//! ---
//! name: review
//! description: 审查一个文件
//! usage: review <path> [focus]
//! args:
//!   - name: path
//!     description: 目标文件
//!     required: true
//!   - name: focus
//!     description: 关注点
//!     default: correctness
//! ---
//! 请审查 {{path}}，重点关注 {{focus}}。
//! ```
//!
//! ```markdown
//! ---
//! name: check-file
//! description: 对单个文件跑 rustfmt 检查
//! args:
//!   - name: path
//!     required: true
//! run: "rustfmt --check {{path}}"
//! ---
//! ```
//!
//! ## 目录与优先级
//!
//! 依次加载 `~/.config/sisyphus/commands` 与 `<project>/.sisyphus/commands`，
//! 同名命令以**项目级覆盖用户级**；文件名（去扩展名）在没有 `name` 时作为命令名。
//!
//! 解析失败的文件会被跳过（不中断其余命令加载）。
//!
//! ## 占位符
//!
//! `required` 缺省时：带 `default` 的参数视为可选，否则必填（显式写 `required` 可覆盖）。
//!
//! 仅替换 frontmatter 中**声明过**的参数；未声明的 `{{...}}` 原样保留，
//! 便于模板里书写 `${{ }}` 之类的非参数文本。必填参数缺失时 `render` 返回错误，
//! 可选参数未提供时使用 `default`，没有 `default` 则替换为空串。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 一个命令参数的声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandArg {
    /// 参数名（即占位符 `{{name}}`）。
    pub name: String,
    /// 参数说明。
    pub description: String,
    /// 是否必填。
    pub required: bool,
    /// 可选参数的默认值。
    pub default: Option<String>,
}

/// 命令形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// 提示词模板（渲染给代理或人）。
    Prompt,
    /// Shell 命令模板（渲染后可执行）。
    Shell,
}

impl CommandKind {
    /// 稳定字符串表示。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Shell => "shell",
        }
    }
}

/// 一个已解析的自定义命令。
#[derive(Debug, Clone)]
pub struct CustomCommand {
    /// 命令名（`command run <name>`）。
    pub name: String,
    /// 一句话说明。
    pub description: String,
    /// 用法字符串（缺省时按参数自动生成）。
    pub usage: Option<String>,
    /// 参数声明。
    pub args: Vec<CommandArg>,
    /// 命令形态。
    pub kind: CommandKind,
    /// 提示词正文或 Shell 命令模板。
    pub template: String,
    /// 来源文件。
    pub source_path: PathBuf,
}

impl CustomCommand {
    /// 实际使用的用法字符串。
    pub fn usage_line(&self) -> String {
        if let Some(usage) = self.usage.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            return usage.to_string();
        }
        if self.args.is_empty() {
            return self.name.clone();
        }
        let args = self
            .args
            .iter()
            .map(|arg| {
                if arg.required {
                    format!("<{}>", arg.name)
                } else {
                    format!("[{}]", arg.name)
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        format!("{} {args}", self.name)
    }

    /// 渲染模板：按声明补全参数并替换 `{{name}}`。
    ///
    /// 返回渲染后的文本；必填参数缺失时返回可读错误（含用法）。
    pub fn render(&self, provided: &HashMap<String, String>) -> Result<String, String> {
        let mut values: Vec<(&str, String)> = Vec::with_capacity(self.args.len());
        for arg in &self.args {
            let value = match provided.get(&arg.name) {
                Some(value) => value.clone(),
                None => match arg.default.as_deref() {
                    Some(default) => default.to_string(),
                    None if arg.required => {
                        return Err(format!(
                            "缺少必填参数 `{}`（用法：{}）",
                            arg.name,
                            self.usage_line()
                        ))
                    },
                    None => String::new(),
                },
            };
            values.push((arg.name.as_str(), value));
        }

        let mut rendered = self.template.clone();
        for (name, value) in values {
            rendered = rendered.replace(&format!("{{{{{name}}}}}"), &value);
        }
        Ok(rendered)
    }

    /// 已声明参数的名称列表（用于展示与补全）。
    pub fn arg_names(&self) -> Vec<&str> {
        self.args.iter().map(|arg| arg.name.as_str()).collect()
    }
}

/// 多目录命令加载器。
#[derive(Debug, Default)]
pub struct CustomCommandLoader {
    dirs: Vec<PathBuf>,
}

impl CustomCommandLoader {
    /// 空加载器。
    pub fn new() -> Self {
        Self { dirs: Vec::new() }
    }

    /// 追加用户级目录 `~/.config/sisyphus/commands`（无法确定 HOME 时忽略）。
    pub fn with_user_dir(mut self) -> Self {
        if let Some(home) = home_dir() {
            self.dirs.push(home.join(".config").join("sisyphus").join("commands"));
        }
        self
    }

    /// 追加项目级目录 `<root>/.sisyphus/commands`。
    pub fn with_project_dir(mut self, root: &Path) -> Self {
        self.dirs.push(root.join(".sisyphus").join("commands"));
        self
    }

    /// 追加任意目录（可多次调用，后加入者优先级更高）。
    pub fn with_dir(mut self, dir: PathBuf) -> Self {
        self.dirs.push(dir);
        self
    }

    /// 加载全部命令：同名时后加载的目录覆盖先加载的目录，结果按名称排序。
    pub fn load_all(&self) -> Vec<CustomCommand> {
        let mut by_name: HashMap<String, CustomCommand> = HashMap::new();
        for dir in &self.dirs {
            if !dir.is_dir() {
                continue;
            }
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|ext| ext != "md") {
                    continue;
                }
                if let Some(command) = parse_command_file(&path) {
                    by_name.insert(command.name.clone(), command);
                }
            }
        }
        let mut commands: Vec<CustomCommand> = by_name.into_values().collect();
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        commands
    }

    /// 按名称查找（同名取高优先级目录中的那份）。
    pub fn find(&self, name: &str) -> Option<CustomCommand> {
        self.load_all().into_iter().find(|command| command.name == name)
    }
}

/// 便捷入口：用户级 + 项目级目录中的全部命令。
pub fn discover_commands(project_root: &Path) -> Vec<CustomCommand> {
    CustomCommandLoader::new().with_user_dir().with_project_dir(project_root).load_all()
}

/// 解析一个命令文件；frontmatter 非法或模板为空时返回 `None`。
pub fn parse_command_file(path: &Path) -> Option<CustomCommand> {
    let content = fs::read_to_string(path).ok()?;
    let (meta, body) = split_frontmatter(&content);
    let meta = meta?;

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("command");
    let name =
        meta.name.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(stem).to_string();

    let run = meta.run.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let body = body.trim();
    let (kind, template) = match run {
        Some(run) => (CommandKind::Shell, run.to_string()),
        None if !body.is_empty() => (CommandKind::Prompt, body.to_string()),
        None => return None,
    };

    let args = meta
        .args
        .unwrap_or_default()
        .into_iter()
        .filter_map(|raw| {
            let name = raw.name?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            Some(CommandArg {
                name,
                description: raw.description.unwrap_or_default().trim().to_string(),
                // 有默认值的参数默认视为可选；显式 `required: true` 仍可覆盖。
                required: raw.required.unwrap_or(raw.default.is_none()),
                default: raw.default,
            })
        })
        .collect();

    Some(CustomCommand {
        name,
        description: meta.description.unwrap_or_default().trim().to_string(),
        usage: meta.usage,
        args,
        kind,
        template,
        source_path: path.to_path_buf(),
    })
}

// ── frontmatter ───────────────────────────────────────────────

#[derive(Debug, Default, serde::Deserialize)]
struct RawCommandMeta {
    name: Option<String>,
    description: Option<String>,
    usage: Option<String>,
    #[serde(default)]
    args: Option<Vec<RawArg>>,
    run: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawArg {
    name: Option<String>,
    description: Option<String>,
    required: Option<bool>,
    default: Option<String>,
}

/// 拆分 YAML frontmatter；非法 frontmatter 时返回 `None`（整份文件跳过）。
fn split_frontmatter(content: &str) -> (Option<RawCommandMeta>, &str) {
    let content = content.trim();
    if !content.starts_with("---") {
        return (None, content);
    }
    let Some(end) = content[3..].find("---").map(|pos| pos + 3) else {
        return (None, content);
    };
    let yaml = content[3..end].trim();
    let body = &content[end + 3..];
    match serde_yaml::from_str::<RawCommandMeta>(yaml) {
        Ok(meta) => (Some(meta), body),
        Err(_) => (None, content),
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, file: &str, content: &str) -> PathBuf {
        let path = dir.join(file);
        fs::write(&path, content).expect("write command file");
        path
    }

    #[test]
    fn parses_prompt_command_with_args() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(
            dir.path(),
            "review.md",
            r#"---
name: review
description: 审查文件
usage: review <path> [focus]
args:
  - name: path
    description: 目标文件
    required: true
  - name: focus
    description: 关注点
    default: correctness
---
请审查 {{path}}，重点关注 {{focus}}。
"#,
        );

        let command = parse_command_file(&path).expect("parse");
        assert_eq!(command.name, "review");
        assert_eq!(command.kind, CommandKind::Prompt);
        assert_eq!(command.usage_line(), "review <path> [focus]");
        assert_eq!(command.arg_names(), vec!["path", "focus"]);
        assert!(command.template.contains("请审查 {{path}}"));
        let focus = command.args.iter().find(|a| a.name == "focus").expect("focus arg");
        assert!(!focus.required, "有 default 的参数默认视为可选");
        let path_arg = command.args.iter().find(|a| a.name == "path").expect("path arg");
        assert!(path_arg.required, "无 default 的参数默认必填");
    }

    #[test]
    fn parses_shell_command_with_run_template() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(
            dir.path(),
            "fmt.md",
            r#"---
name: check-file
args:
  - name: path
    required: true
run: "rustfmt --check {{path}}"
---
正文被忽略。
"#,
        );
        let command = parse_command_file(&path).expect("parse");
        assert_eq!(command.kind, CommandKind::Shell);
        assert_eq!(command.template, "rustfmt --check {{path}}");
        assert_eq!(command.args.len(), 1);
    }

    #[test]
    fn render_substitutes_required_and_default_args() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(
            dir.path(),
            "review.md",
            r#"---
name: review
args:
  - name: path
    required: true
  - name: focus
    default: correctness
---
review {{path}} for {{focus}}
"#,
        );
        let command = parse_command_file(&path).expect("parse");

        let mut provided = HashMap::new();
        provided.insert("path".to_string(), "src/lib.rs".to_string());
        let rendered = command.render(&provided).expect("render");
        assert_eq!(rendered, "review src/lib.rs for correctness");

        provided.insert("focus".to_string(), "security".to_string());
        let rendered = command.render(&provided).expect("render");
        assert_eq!(rendered, "review src/lib.rs for security");
    }

    #[test]
    fn render_rejects_missing_required_arg_with_usage() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(
            dir.path(),
            "review.md",
            r#"---
name: review
args:
  - name: path
    required: true
---
{{path}}
"#,
        );
        let command = parse_command_file(&path).expect("parse");
        let err = command.render(&HashMap::new()).expect_err("must fail");
        assert!(err.contains("缺少必填参数 `path`"), "err={err}");
        assert!(err.contains("review <path>"), "err={err}");
    }

    #[test]
    fn render_replaces_optional_without_default_by_empty_string() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(
            dir.path(),
            "opt.md",
            r#"---
name: opt
args:
  - name: note
    required: false
---
start{{note}}end
"#,
        );
        let command = parse_command_file(&path).expect("parse");
        assert_eq!(command.render(&HashMap::new()).expect("render"), "startend");
    }

    #[test]
    fn render_keeps_undeclared_placeholders() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(
            dir.path(),
            "raw.md",
            r#"---
name: raw
---
keep ${{ github.ref }} and {{unknown}}
"#,
        );
        let command = parse_command_file(&path).expect("parse");
        let rendered = command.render(&HashMap::new()).expect("render");
        assert!(rendered.contains("${{ github.ref }}"));
        assert!(rendered.contains("{{unknown}}"));
    }

    #[test]
    fn loader_prefers_later_dirs_for_same_name() {
        let user = TempDir::new().expect("tmpdir");
        let project = TempDir::new().expect("tmpdir");
        write(
            user.path(),
            "deploy.md",
            "---\nname: deploy\ndescription: user\n---\nuser body\n",
        );
        write(
            project.path(),
            "deploy.md",
            "---\nname: deploy\ndescription: project\n---\nproject body\n",
        );

        let commands = CustomCommandLoader::new()
            .with_dir(user.path().to_path_buf())
            .with_dir(project.path().to_path_buf())
            .load_all();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].description, "project");
        assert_eq!(commands[0].template, "project body");
    }

    #[test]
    fn loader_sorts_by_name_and_skips_invalid_files() {
        let dir = TempDir::new().expect("tmpdir");
        write(dir.path(), "b.md", "---\nname: bravo\n---\nb\n");
        write(dir.path(), "a.md", "---\nname: alpha\n---\na\n");
        write(dir.path(), "broken.md", "---\nname: [unclosed\n---\nx\n");
        write(dir.path(), "empty.md", "---\nname: empty\n---\n\n");
        fs::write(dir.path().join("notes.txt"), "ignored").expect("write");

        let commands = CustomCommandLoader::new().with_dir(dir.path().to_path_buf()).load_all();
        let names: Vec<_> = commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "bravo"], "非法/空模板文件应被跳过");
    }

    #[test]
    fn parse_without_frontmatter_is_skipped() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(dir.path(), "plain.md", "just markdown\n");
        assert!(parse_command_file(&path).is_none());
    }

    #[test]
    fn name_defaults_to_file_stem() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(dir.path(), "cleanup.md", "---\ndescription: x\n---\nbody\n");
        let command = parse_command_file(&path).expect("parse");
        assert_eq!(command.name, "cleanup");
    }

    #[test]
    fn usage_line_generated_from_args_when_missing() {
        let dir = TempDir::new().expect("tmpdir");
        let path = write(
            dir.path(),
            "auto.md",
            r#"---
name: auto
args:
  - name: a
    required: true
  - name: b
    required: false
---
{{a}} {{b}}
"#,
        );
        let command = parse_command_file(&path).expect("parse");
        assert_eq!(command.usage_line(), "auto <a> [b]");
    }
}
