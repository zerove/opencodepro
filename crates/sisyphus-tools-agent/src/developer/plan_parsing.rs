//! 宽松的 `DevelopmentPlan` 解析（弱模型兼容）。
//!
//! 严格 `serde` 解析要求模型给出**每个**字段：缺 `goal`、把动作类型当验证类型
//! （`{"type":"ReadFile"}` 作为 verification）、`params` 键名写成 `file` 而不是
//! `path`…… 任何一个不合规都会让**整份计划**作废，agent 直接失败 —— 而这些
//! 字段大多与「做什么」无关：
//!
//! - `goal` / `overview`：引擎自己知道目标，模型漏写不该让计划作废；
//! - `description` / `expected_outcome`：只是给人看的说明；
//! - `status`：本来就该是 `Pending`；
//! - `verification`：步骤级自检，**不是**目标判据（目标判据是 `--verify-cmd`
//!   的客观闸门），未知类型退化成 `None` 不会让任务判错。
//!
//! 因此这里提供一套**兜底**解析：严格解析失败时用它，尽量把模型的意思还原成
//! 可执行的计划。动作本身（要改哪个文件、跑什么命令）仍必须能识别 —— 认不出
//! 就返回错误，不猜。

use serde_json::Value;

use super::types::{DevAction, DevStep, DevelopmentPlan, StepStatus, VerificationMethod};

/// 宽松解析一份计划 JSON。
///
/// `fallback_goal` 用于模型没写 `goal` 时（引擎已知目标）。
pub fn parse_plan_lenient(raw: &str, fallback_goal: &str) -> Result<DevelopmentPlan, String> {
    let value: Value = serde_json::from_str(raw.trim())
        .map_err(|e| format!("宽松解析失败（不是合法 JSON）: {e}"))?;

    let steps_value = value
        .get("steps")
        .and_then(|s| s.as_array())
        .ok_or_else(|| "计划里没有 steps 数组".to_string())?;
    if steps_value.is_empty() {
        return Err("计划的 steps 为空".to_string());
    }

    let mut steps = Vec::with_capacity(steps_value.len());
    for (idx, raw_step) in steps_value.iter().enumerate() {
        let action = parse_action(raw_step.get("action").unwrap_or(&Value::Null))?;
        steps.push(DevStep {
            id: raw_step.get("id").and_then(|v| v.as_u64()).map(|v| v as usize).unwrap_or(idx + 1),
            description: string_field(raw_step, &["description", "desc", "title"]),
            action,
            expected_outcome: string_field(raw_step, &["expected_outcome", "expected"]),
            verification: parse_verification(raw_step.get("verification")),
            status: StepStatus::Pending,
        });
    }

    let goal = {
        let g = string_field(&value, &["goal", "objective", "task"]);
        if g.is_empty() {
            fallback_goal.to_string()
        } else {
            g
        }
    };

    Ok(DevelopmentPlan {
        goal,
        overview: string_field(&value, &["overview", "summary", "description"]),
        steps,
    })
}

/// 读取第一个存在且是字符串的字段。
fn string_field(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(s) = value.get(*key).and_then(|v| v.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}

/// 解析动作（认不出就报错，不猜）。
fn parse_action(value: &Value) -> Result<DevAction, String> {
    // 兼容两种形态：`{"type": "...", "params": {...}}` 与
    // `{"type": "...", ...字段平铺}`。
    let action_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "动作缺少 type 字段".to_string())?;
    let params = value.get("params").unwrap_or(value);

    match action_type {
        "CreateFile" | "create_file" | "WriteFile" | "write_file" => Ok(DevAction::CreateFile {
            path: path_of(params),
            content: params
                .get("content")
                .or_else(|| params.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
        "EditFile" | "edit_file" | "ReplaceInFile" => Ok(DevAction::EditFile {
            path: path_of(params),
            old_string: params
                .get("old_string")
                .or_else(|| params.get("old"))
                .or_else(|| params.get("oldString"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            new_string: params
                .get("new_string")
                .or_else(|| params.get("new"))
                .or_else(|| params.get("newString"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
        "RunCommand" | "run_command" | "Exec" | "Bash" => Ok(DevAction::RunCommand {
            command: params
                .get("command")
                .or_else(|| params.get("cmd"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            dir: params
                .get("dir")
                .or_else(|| params.get("directory"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }),
        // `OpenFile` 是模型常用的「打开看看」说法，与 ReadFile 同义。
        "ReadFile" | "read_file" | "OpenFile" | "open_file" | "ViewFile" => {
            Ok(DevAction::ReadFile {
                path: path_of(params),
            })
        },
        "ComplexChange" | "complex_change" => Ok(DevAction::ComplexChange {
            description: params
                .get("description")
                .or_else(|| params.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
        other => Err(format!("无法识别的动作类型: {other}")),
    }
}

/// 文件路径（兼容 `path` / `file` / `file_path`）。
fn path_of(params: &Value) -> String {
    params
        .get("path")
        .or_else(|| params.get("file"))
        .or_else(|| params.get("file_path"))
        .or_else(|| params.get("filename"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// 解析验证方式；**未知类型退化为 `None`**（步骤级自检不是目标判据）。
fn parse_verification(value: Option<&Value>) -> VerificationMethod {
    let Some(value) = value else {
        return VerificationMethod::None;
    };
    let Some(kind) = value.get("type").and_then(|v| v.as_str()) else {
        return VerificationMethod::None;
    };
    match kind {
        "CargoCheck" => VerificationMethod::CargoCheck,
        "CargoTest" => VerificationMethod::CargoTest,
        "LspDiagnostics" => VerificationMethod::LspDiagnostics {
            path: value.get("path").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        },
        "Command" => VerificationMethod::Command {
            command: value.get("command").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        },
        "LlmReview" => VerificationMethod::LlmReview {
            criteria: value
                .get("criteria")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        },
        _ => VerificationMethod::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plan_without_goal_and_overview() {
        let raw = r#"{"steps":[{"id":1,"action":{"type":"CreateFile","params":{"path":"a.txt","content":"x"}},"verification":{"type":"None"}}]}"#;
        let plan = parse_plan_lenient(raw, "fallback goal").expect("应可解析");
        assert_eq!(plan.goal, "fallback goal");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].id, 1);
        assert!(plan.steps[0].description.is_empty());
    }

    #[test]
    fn unknown_verification_type_degrades_to_none() {
        // 实测弱模型会把动作类型当验证类型（`ReadFile`）。
        let raw = r#"{"goal":"g","overview":"o","steps":[{"id":1,"description":"d","action":{"type":"EditFile","params":{"path":"a.rs","old_string":"x","new_string":"y"}},"expected_outcome":"e","verification":{"type":"ReadFile","params":{"path":"a.rs"}},"status":"Pending"}]}"#;
        let plan = parse_plan_lenient(raw, "fallback").expect("应可解析");
        assert!(matches!(
            plan.steps[0].verification,
            VerificationMethod::None
        ));
    }

    #[test]
    fn accepts_flat_params_and_alias_keys() {
        let raw = r#"{"goal":"g","steps":[{"action":{"type":"CreateFile","file":"a.txt","content":"x"}}]}"#;
        let plan = parse_plan_lenient(raw, "fallback").expect("应可解析");
        match &plan.steps[0].action {
            DevAction::CreateFile { path, content } => {
                assert_eq!(path, "a.txt");
                assert_eq!(content, "x");
            },
            other => panic!("动作类型不对: {other:?}"),
        }
    }

    #[test]
    fn unknown_action_type_is_an_error() {
        let raw = r#"{"goal":"g","steps":[{"action":{"type":"Deploy","params":{}}}]}"#;
        assert!(parse_plan_lenient(raw, "fallback").is_err());
    }

    #[test]
    fn missing_steps_is_an_error() {
        assert!(parse_plan_lenient(r#"{"goal":"g"}"#, "fallback").is_err());
        assert!(parse_plan_lenient(r#"{"goal":"g","steps":[]}"#, "fallback").is_err());
        assert!(parse_plan_lenient("not json", "fallback").is_err());
    }
}
