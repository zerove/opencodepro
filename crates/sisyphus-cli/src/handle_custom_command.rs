//! `sisyphus command`：自定义开发命令（`.sisyphus/commands/*.md`）的发现与执行。
//!
//! 子命令：
//! - `list`：列出用户级 + 项目级命令；
//! - `show`：查看命令参数与模板；
//! - `run`：渲染模板；Shell 命令可加 `--exec` 执行（走统一权限检查）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::commands::CommandCommands;
use sisyphus_cli::permission::CliPermission;
use sisyphus_cli::tools::custom_command::{discover_commands, CommandKind, CustomCommand};

/// 分发 `command` 子命令。
pub async fn handle_command(sub: &CommandCommands, perms: &CliPermission) -> Result<()> {
    match sub {
        CommandCommands::List { json, cwd } => {
            let commands = discover(cwd.as_deref());
            if *json {
                let payload: Vec<serde_json::Value> = commands.iter().map(command_json).collect();
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else if commands.is_empty() {
                println!(
                    "No custom commands. Add markdown files under .sisyphus/commands/ (project) \
                     or ~/.config/sisyphus/commands/ (user)."
                );
            } else {
                for command in &commands {
                    println!(
                        "{:<16} {:<7} {:<28} {}",
                        command.name,
                        command.kind.as_str(),
                        command.usage_line(),
                        command.description
                    );
                }
            }
            Ok(())
        },
        CommandCommands::Show { name, json, cwd } => {
            let commands = discover(cwd.as_deref());
            let Some(command) = commands.iter().find(|c| &c.name == name) else {
                anyhow::bail!("未找到命令 `{name}`（使用 `sisyphus command list` 查看可用命令）");
            };
            if *json {
                println!("{}", serde_json::to_string_pretty(&command_json(command))?);
            } else {
                println!("{}", format_command(command));
            }
            Ok(())
        },
        CommandCommands::Run {
            name,
            args,
            exec,
            timeout_ms,
            json,
            cwd,
        } => {
            let commands = discover(cwd.as_deref());
            let Some(command) = commands.iter().find(|c| &c.name == name) else {
                anyhow::bail!("未找到命令 `{name}`（使用 `sisyphus command list` 查看可用命令）");
            };
            let provided = parse_args(args)?;
            let rendered = command.render(&provided).map_err(anyhow::Error::msg)?;

            if !exec {
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "name": command.name,
                            "kind": command.kind.as_str(),
                            "rendered": rendered,
                            "executed": false,
                        }))?
                    );
                } else {
                    print!("{rendered}");
                    if !rendered.ends_with('\n') {
                        println!();
                    }
                }
                return Ok(());
            }

            if command.kind != CommandKind::Shell {
                anyhow::bail!(
                    "`{}` 是提示词命令（kind=prompt），不能加 --exec；去掉 --exec 可查看渲染结果",
                    command.name
                );
            }

            let output = {
                // 与 `sisyphus exec` 共用审批/审计口径
                sisyphus_cli::permission::require_tool(perms, "exec")?;
                sisyphus_cli::tools::bash::execute(&rendered, None, *timeout_ms).await?
            };
            if output.exit_code != 0 {
                anyhow::bail!(
                    "命令 `{}` 退出码 {}: {}",
                    command.name,
                    output.exit_code,
                    output.stderr.trim()
                );
            }
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "name": command.name,
                        "kind": command.kind.as_str(),
                        "rendered": rendered,
                        "executed": true,
                        "exit_code": output.exit_code,
                        "stdout": output.stdout,
                        "stderr": output.stderr,
                    }))?
                );
            } else {
                print!("{}", output.stdout);
                if !output.stderr.is_empty() {
                    eprint!("{}", output.stderr);
                }
            }
            Ok(())
        },
    }
}

fn discover(cwd: Option<&Path>) -> Vec<CustomCommand> {
    let root: PathBuf = cwd.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    discover_commands(&root)
}

/// 解析 `key=value` 参数；缺少 `=` 视为错误（避免把拼写错误当成空值）。
fn parse_args(raw: &[String]) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for item in raw {
        let (key, value) = item
            .split_once('=')
            .with_context(|| format!("参数 `{item}` 缺少 `=`（期望 key=value）"))?;
        let key = key.trim();
        if key.is_empty() {
            anyhow::bail!("参数名不能为空（输入：`{item}`）");
        }
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

fn command_json(command: &CustomCommand) -> serde_json::Value {
    let args: Vec<serde_json::Value> = command
        .args
        .iter()
        .map(|arg| {
            serde_json::json!({
                "name": arg.name,
                "description": arg.description,
                "required": arg.required,
                "default": arg.default,
            })
        })
        .collect();
    serde_json::json!({
        "name": command.name,
        "description": command.description,
        "usage": command.usage_line(),
        "kind": command.kind.as_str(),
        "args": args,
        "template": command.template,
        "source": command.source_path.display().to_string(),
    })
}

fn format_command(command: &CustomCommand) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n", command.name));
    if !command.description.is_empty() {
        out.push_str(&format!("  {}\n", command.description));
    }
    out.push_str(&format!("  usage: {}\n", command.usage_line()));
    out.push_str(&format!("  kind:  {}\n", command.kind.as_str()));
    out.push_str(&format!("  source: {}\n", command.source_path.display()));
    if !command.args.is_empty() {
        out.push_str("  args:\n");
        for arg in &command.args {
            let required = if arg.required { "required" } else { "optional" };
            let default =
                arg.default.as_deref().map(|d| format!(", default={d}")).unwrap_or_default();
            out.push_str(&format!(
                "    - {} ({required}{default}) {}\n",
                arg.name, arg.description
            ));
        }
    }
    out.push_str(&format!(
        "  template:\n{}\n",
        indent(&command.template, "    ")
    ));
    out
}

fn indent(text: &str, prefix: &str) -> String {
    text.lines().map(|line| format!("{prefix}{line}")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_accepts_pairs_and_values_with_equals() {
        let parsed = parse_args(&["path=src/lib.rs".into(), "expr=a=b".into()]).expect("parse");
        assert_eq!(parsed.get("path").map(String::as_str), Some("src/lib.rs"));
        assert_eq!(parsed.get("expr").map(String::as_str), Some("a=b"));
    }

    #[test]
    fn parse_args_rejects_missing_equals_and_empty_key() {
        let err = parse_args(&["path".into()]).expect_err("must fail");
        assert!(err.to_string().contains("缺少 `=`"), "err={err}");
        let err = parse_args(&["=value".into()]).expect_err("must fail");
        assert!(err.to_string().contains("参数名不能为空"), "err={err}");
    }

    #[test]
    fn format_command_lists_args_and_indents_template() {
        use sisyphus_cli::tools::custom_command::{CommandArg, CustomCommand};
        let command = CustomCommand {
            name: "review".into(),
            description: "审查文件".into(),
            usage: None,
            args: vec![CommandArg {
                name: "path".into(),
                description: "目标文件".into(),
                required: true,
                default: None,
            }],
            kind: CommandKind::Prompt,
            template: "line1\nline2".into(),
            source_path: PathBuf::from("/tmp/review.md"),
        };
        let text = format_command(&command);
        assert!(text.contains("usage: review <path>"));
        assert!(text.contains("kind:  prompt"));
        assert!(text.contains("    - path (required) 目标文件"));
        assert!(text.contains("    line1\n    line2"));
    }

    #[test]
    fn command_json_exposes_kind_and_args() {
        use sisyphus_cli::tools::custom_command::{CommandArg, CustomCommand};
        let command = CustomCommand {
            name: "fmt".into(),
            description: String::new(),
            usage: Some("fmt <file>".into()),
            args: vec![CommandArg {
                name: "file".into(),
                description: String::new(),
                required: true,
                default: None,
            }],
            kind: CommandKind::Shell,
            template: "rustfmt {{file}}".into(),
            source_path: PathBuf::from("/tmp/fmt.md"),
        };
        let json = command_json(&command);
        assert_eq!(json["kind"], "shell");
        assert_eq!(json["usage"], "fmt <file>");
        assert_eq!(json["args"][0]["name"], "file");
        assert_eq!(json["args"][0]["required"], true);
    }
}
