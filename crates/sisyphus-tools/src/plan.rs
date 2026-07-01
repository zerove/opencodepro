mod types;
pub use types::*;

use crate::ToolResult;
use std::fs;
use std::path::Path;

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days_since_epoch = secs / 86400;
    let year = 1970 + (days_since_epoch as f64 / 365.25) as u64;
    let month = 1 + ((days_since_epoch % 365) as f64 / 30.44) as u64;
    let day = 1 + ((days_since_epoch % 365) as f64 % 30.44) as u64;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Create a new plan.
pub fn plan_create(name: &str, description: &str, steps: Vec<PlanStep>) -> Plan {
    let now = now_iso();
    let id = format!("plan_{:x}", now.len()); // simple unique-ish id
    Plan {
        id,
        name: name.to_string(),
        description: description.to_string(),
        status: PlanStatus::Draft,
        steps,
        created_at: now.clone(),
        updated_at: now.clone(),
    }
}

/// Save a plan to a JSON file.
pub fn plan_save(plan: &Plan, path: &Path) -> ToolResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(crate::ToolError::Io)?;
    }
    let content = serde_json::to_string_pretty(plan)
        .map_err(|e| crate::ToolError::TodoError(format!("serialize: {e}")))?;
    fs::write(path, content).map_err(crate::ToolError::Io)?;
    Ok(())
}

/// Load a plan from a JSON file.
pub fn plan_load(path: &Path) -> ToolResult<Plan> {
    let content = fs::read_to_string(path).map_err(crate::ToolError::Io)?;
    let plan: Plan = serde_json::from_str(&content)
        .map_err(|e| crate::ToolError::TodoError(format!("deserialize: {e}")))?;
    Ok(plan)
}

/// Review a plan for completeness and consistency.
/// Returns a list of review comments (issues found).
pub fn plan_review(plan: &Plan) -> Vec<String> {
    let mut comments = vec![];

    if plan.name.is_empty() {
        comments.push("Plan name is empty".to_string());
    }
    if plan.description.is_empty() {
        comments.push("Plan description is empty".to_string());
    }
    if plan.steps.is_empty() {
        comments.push("Plan has no steps".to_string());
    }

    for step in &plan.steps {
        if step.description.is_empty() {
            comments.push(format!("Step '{}' has empty description", step.id));
        }
        for dep in &step.dependencies {
            if !plan.steps.iter().any(|s| s.id == *dep) {
                comments.push(format!(
                    "Step '{}' depends on '{}' which does not exist",
                    step.id, dep
                ));
            }
        }
    }

    comments
}

/// Approve a plan (change status to Approved).
pub fn plan_approve(plan: &mut Plan) {
    plan.status = PlanStatus::Approved;
    plan.updated_at = now_iso();
}

/// Cancel a plan.
pub fn plan_cancel(plan: &mut Plan) {
    plan.status = PlanStatus::Cancelled;
    plan.updated_at = now_iso();
}

/// Create a simple plan from a requirements string (line-based parsing).
/// Lines starting with "- " or "* " are treated as steps.
pub fn plan_from_requirements(name: &str, requirements: &str) -> Plan {
    let mut steps = vec![];
    for (i, line) in requirements.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let desc = trimmed[2..].trim().to_string();
            if !desc.is_empty() {
                steps.push(PlanStep {
                    id: format!("step_{}", i + 1),
                    description: desc,
                    status: PlanStatus::Draft,
                    dependencies: vec![],
                    estimated_effort: None,
                });
            }
        }
    }

    // If no structured steps found, treat the whole text as a single step
    if steps.is_empty() && !requirements.trim().is_empty() {
        steps.push(PlanStep {
            id: "step_1".to_string(),
            description: requirements.trim().to_string(),
            status: PlanStatus::Draft,
            dependencies: vec![],
            estimated_effort: None,
        });
    }

    plan_create(name, requirements, steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_plan() -> Plan {
        let steps = vec![
            PlanStep {
                id: "step_1".to_string(),
                description: "Implement core logic".to_string(),
                status: PlanStatus::Draft,
                dependencies: vec![],
                estimated_effort: Some("2d".to_string()),
            },
            PlanStep {
                id: "step_2".to_string(),
                description: "Write tests".to_string(),
                status: PlanStatus::Draft,
                dependencies: vec!["step_1".to_string()],
                estimated_effort: Some("1d".to_string()),
            },
        ];
        plan_create("Test Plan", "A test plan", steps)
    }

    #[test]
    fn test_plan_create() {
        let plan = sample_plan();
        assert_eq!(plan.name, "Test Plan");
        assert_eq!(plan.status, PlanStatus::Draft);
        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn test_plan_save_and_load() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("plan.json");
        let plan = sample_plan();
        plan_save(&plan, &path).unwrap();

        let loaded = plan_load(&path).unwrap();
        assert_eq!(loaded.name, plan.name);
        assert_eq!(loaded.steps.len(), plan.steps.len());
        assert_eq!(loaded.steps[0].description, plan.steps[0].description);
    }

    #[test]
    fn test_plan_review_clean() {
        let plan = sample_plan();
        let comments = plan_review(&plan);
        assert!(
            comments.is_empty(),
            "expected no comments, got: {:?}",
            comments
        );
    }

    #[test]
    fn test_plan_review_empty_name() {
        let mut plan = sample_plan();
        plan.name = "".to_string();
        let comments = plan_review(&plan);
        assert!(comments.iter().any(|c| c.contains("name is empty")));
    }

    #[test]
    fn test_plan_review_empty_steps() {
        let plan = plan_create("Empty", "No steps", vec![]);
        let comments = plan_review(&plan);
        assert!(comments.iter().any(|c| c.contains("no steps")));
    }

    #[test]
    fn test_plan_review_missing_dependency() {
        let steps = vec![PlanStep {
            id: "step_1".to_string(),
            description: "Do work".to_string(),
            status: PlanStatus::Draft,
            dependencies: vec!["step_999".to_string()],
            estimated_effort: None,
        }];
        let plan = plan_create("Bad Dep", "Missing dep test", steps);
        let comments = plan_review(&plan);
        assert!(comments.iter().any(|c| c.contains("step_999")));
    }

    #[test]
    fn test_plan_approve() {
        let mut plan = sample_plan();
        plan_approve(&mut plan);
        assert_eq!(plan.status, PlanStatus::Approved);
    }

    #[test]
    fn test_plan_cancel() {
        let mut plan = sample_plan();
        plan_cancel(&mut plan);
        assert_eq!(plan.status, PlanStatus::Cancelled);
    }

    #[test]
    fn test_plan_from_requirements() {
        let reqs = "\
- Implement authentication
- Add database layer
- Write API endpoints
";
        let plan = plan_from_requirements("Auth System", reqs);
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].description, "Implement authentication");
        assert_eq!(plan.steps[1].description, "Add database layer");
    }

    #[test]
    fn test_plan_from_requirements_asterisk() {
        let reqs = "* Setup CI\n* Add monitoring\n";
        let plan = plan_from_requirements("Infra", reqs);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].description, "Setup CI");
    }

    #[test]
    fn test_plan_from_requirements_fallback() {
        let reqs = "Just build the whole thing in one step";
        let plan = plan_from_requirements("Single", reqs);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(
            plan.steps[0].description,
            "Just build the whole thing in one step"
        );
    }

    #[test]
    fn test_plan_display_status() {
        assert_eq!(PlanStatus::Draft.to_string(), "draft");
        assert_eq!(PlanStatus::Reviewed.to_string(), "reviewed");
        assert_eq!(PlanStatus::Approved.to_string(), "approved");
        assert_eq!(PlanStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn test_plan_step_dependencies() {
        let plan = sample_plan();
        assert!(plan.steps[0].dependencies.is_empty());
        assert_eq!(plan.steps[1].dependencies, vec!["step_1"]);
    }

    #[test]
    fn test_plan_save_creates_parent_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let nested = dir.path().join("sub").join("nested").join("plan.json");
        let plan = sample_plan();
        plan_save(&plan, &nested).unwrap();
        assert!(
            nested.exists(),
            "plan_save should create parent directories"
        );
    }

    #[test]
    fn test_plan_load_nonexistent() {
        let result = plan_load(Path::new("/tmp/__sisyphus_nonexistent_plan__.json"));
        assert!(result.is_err(), "loading non-existent plan should fail");
    }

    #[test]
    fn test_plan_load_malformed_json() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{this is not json}").unwrap();
        let result = plan_load(&path);
        assert!(result.is_err(), "malformed JSON should fail to load");
    }

    #[test]
    fn test_plan_from_requirements_mixed_prefixes() {
        let reqs = "- First\n* Second\n- Third\n";
        let plan = plan_from_requirements("Mixed", reqs);
        assert_eq!(plan.steps.len(), 3);
    }

    #[test]
    fn test_plan_from_requirements_empty_string() {
        let plan = plan_from_requirements("Empty", "");
        assert_eq!(plan.steps.len(), 0);
    }

    #[test]
    fn test_plan_from_requirements_whitespace_only() {
        let plan = plan_from_requirements("WS", "   \n  \n  ");
        assert_eq!(plan.steps.len(), 0);
    }

    #[test]
    fn test_plan_review_empty_description() {
        let mut plan = sample_plan();
        plan.description = "".to_string();
        let comments = plan_review(&plan);
        assert!(comments.iter().any(|c| c.contains("description is empty")));
    }

    #[test]
    fn test_plan_review_step_empty_description() {
        let steps = vec![PlanStep {
            id: "step_1".to_string(),
            description: "".to_string(),
            status: PlanStatus::Draft,
            dependencies: vec![],
            estimated_effort: None,
        }];
        let plan = plan_create("Bad Step", "Test", steps);
        let comments = plan_review(&plan);
        assert!(comments.iter().any(|c| c.contains("empty description")));
    }

    #[test]
    fn test_plan_approve_already_approved() {
        let mut plan = sample_plan();
        plan_approve(&mut plan);
        plan_approve(&mut plan);
        assert_eq!(plan.status, PlanStatus::Approved);
    }

    #[test]
    fn test_plan_save_and_load_roundtrip_all_fields() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("full.json");

        let steps = vec![
            PlanStep {
                id: "s1".to_string(),
                description: "Design".to_string(),
                status: PlanStatus::Draft,
                dependencies: vec![],
                estimated_effort: Some("3d".to_string()),
            },
            PlanStep {
                id: "s2".to_string(),
                description: "Implement".to_string(),
                status: PlanStatus::Draft,
                dependencies: vec!["s1".to_string()],
                estimated_effort: None,
            },
        ];
        let plan = plan_create("Roundtrip", "Full field test", steps);
        plan_save(&plan, &path).unwrap();

        let loaded = plan_load(&path).unwrap();
        assert_eq!(loaded.id, plan.id);
        assert_eq!(loaded.name, plan.name);
        assert_eq!(loaded.description, plan.description);
        assert_eq!(loaded.status, plan.status);
        assert_eq!(loaded.steps.len(), plan.steps.len());
        assert_eq!(loaded.steps[0].id, plan.steps[0].id);
        assert_eq!(
            loaded.steps[0].estimated_effort,
            plan.steps[0].estimated_effort
        );
        assert_eq!(loaded.steps[1].dependencies, plan.steps[1].dependencies);
        assert_eq!(loaded.created_at, plan.created_at);
        assert_eq!(loaded.updated_at, plan.updated_at);
    }
}
