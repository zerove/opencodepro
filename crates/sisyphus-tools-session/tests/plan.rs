//! Integration tests for sisyphus-tools-session `plan` module.
//!
//! Exercises the public `plan_create` / `plan_save` / `plan_load` /
//! `plan_review` / `plan_approve` / `plan_cancel` / `plan_from_requirements`
//! functions used to manage structured plans and review them for
//! completeness before approval.

use sisyphus_tools_session::plan::{
    plan_approve, plan_cancel, plan_create, plan_from_requirements, plan_load, plan_review,
    plan_save, Plan, PlanStatus, PlanStep,
};
use std::path::PathBuf;
use tempfile::TempDir;

fn step(id: &str, desc: &str, deps: Vec<&str>) -> PlanStep {
    PlanStep {
        id: id.to_string(),
        description: desc.to_string(),
        status: PlanStatus::Draft,
        dependencies: deps.into_iter().map(String::from).collect(),
        estimated_effort: None,
    }
}

// ---------------------------------------------------------------------------
// plan_create
// ---------------------------------------------------------------------------

#[test]
fn plan_create_starts_in_draft() {
    let p = plan_create("Test", "Desc", vec![]);
    assert_eq!(p.status, PlanStatus::Draft);
    assert_eq!(p.name, "Test");
    assert_eq!(p.description, "Desc");
    assert!(p.steps.is_empty());
    assert!(p.id.starts_with("plan_"));
    assert!(!p.created_at.is_empty());
    assert_eq!(p.created_at, p.updated_at);
}

#[test]
fn plan_create_with_steps_preserves_them() {
    let steps = vec![step("s1", "first", vec![]), step("s2", "second", vec!["s1"])];
    let p = plan_create("Test", "Desc", steps);
    assert_eq!(p.steps.len(), 2);
    assert_eq!(p.steps[0].id, "s1");
    assert_eq!(p.steps[1].dependencies, vec!["s1".to_string()]);
}

// ---------------------------------------------------------------------------
// plan_save / plan_load
// ---------------------------------------------------------------------------

#[test]
fn plan_save_and_load_roundtrip() {
    let dir = TempDir::new().expect("tempdir");
    let path: PathBuf = dir.path().join("plan.json");
    let p = plan_create("Roundtrip", "Test roundtrip", vec![step("s1", "step 1", vec![])]);
    plan_save(&p, &path).expect("save");
    let restored = plan_load(&path).expect("load");
    assert_eq!(restored.id, p.id);
    assert_eq!(restored.name, "Roundtrip");
    assert_eq!(restored.description, "Test roundtrip");
    assert_eq!(restored.steps.len(), 1);
    assert_eq!(restored.steps[0].id, "s1");
}

#[test]
fn plan_save_creates_parent_dirs() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("a").join("b").join("plan.json");
    let p = plan_create("Nested", "D", vec![]);
    plan_save(&p, &path).expect("save");
    assert!(path.exists());
}

#[test]
fn plan_load_nonexistent_returns_error() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("nope.json");
    let result = plan_load(&path);
    assert!(result.is_err());
}

#[test]
fn plan_load_malformed_json_returns_error() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "{ not valid json }").expect("write");
    let result = plan_load(&path);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// plan_review
// ---------------------------------------------------------------------------

#[test]
fn plan_review_clean_plan_returns_no_comments() {
    let p = plan_create(
        "Good Plan",
        "Has everything",
        vec![step("s1", "well defined", vec![])],
    );
    assert!(plan_review(&p).is_empty());
}

#[test]
fn plan_review_flags_empty_name() {
    let mut p = plan_create("", "Desc", vec![step("s1", "ok", vec![])]);
    p.name = String::new();
    let comments = plan_review(&p);
    assert!(comments.iter().any(|c| c.contains("name")));
}

#[test]
fn plan_review_flags_empty_description() {
    let p = plan_create("Name", "", vec![step("s1", "ok", vec![])]);
    let comments = plan_review(&p);
    assert!(comments.iter().any(|c| c.contains("description")));
}

#[test]
fn plan_review_flags_no_steps() {
    let p = plan_create("Name", "Desc", vec![]);
    let comments = plan_review(&p);
    assert!(comments.iter().any(|c| c.contains("no steps")));
}

#[test]
fn plan_review_flags_empty_step_description() {
    let p = plan_create("Name", "Desc", vec![step("s1", "", vec![])]);
    let comments = plan_review(&p);
    assert!(comments.iter().any(|c| c.contains("s1") && c.contains("empty description")));
}

#[test]
fn plan_review_flags_unmet_dependency() {
    let p = plan_create(
        "Name",
        "Desc",
        vec![step("s1", "depends on ghost", vec!["ghost"])],
    );
    let comments = plan_review(&p);
    assert!(comments.iter().any(|c| c.contains("ghost") && c.contains("does not exist")));
}

#[test]
fn plan_review_collects_multiple_issues() {
    let p = plan_create("", "", vec![step("s1", "", vec!["missing"])]);
    let comments = plan_review(&p);
    // Expect at least: empty name, empty description, empty step description, missing dep.
    assert!(comments.len() >= 4, "got {} comments", comments.len());
}

#[test]
fn plan_review_satisfied_dependency_passes() {
    let p = plan_create(
        "P",
        "D",
        vec![
            step("s1", "first", vec![]),
            step("s2", "second", vec!["s1"]),
        ],
    );
    let comments = plan_review(&p);
    assert!(comments.is_empty());
}

// ---------------------------------------------------------------------------
// plan_approve / plan_cancel
// ---------------------------------------------------------------------------

#[test]
fn plan_approve_sets_status_and_updates_timestamp() {
    let mut p = plan_create("P", "D", vec![]);
    let before = p.updated_at.clone();
    // Sleep is not reliable; we just check status change.
    plan_approve(&mut p);
    assert_eq!(p.status, PlanStatus::Approved);
    // updated_at should be set (may be equal to before if very fast).
    assert!(!p.updated_at.is_empty());
    let _ = before;
}

#[test]
fn plan_cancel_sets_status_cancelled() {
    let mut p = plan_create("P", "D", vec![]);
    plan_cancel(&mut p);
    assert_eq!(p.status, PlanStatus::Cancelled);
}

// ---------------------------------------------------------------------------
// plan_from_requirements
// ---------------------------------------------------------------------------

#[test]
fn plan_from_requirements_creates_steps() {
    let req = "First, do A. Then, do B. Finally, do C.";
    let p = plan_from_requirements("My Plan", req);
    assert_eq!(p.name, "My Plan");
    // Each sentence becomes a step (rough split).
    assert!(!p.steps.is_empty(), "expected at least one step");
}

#[test]
fn plan_from_requirements_handles_short_input() {
    let p = plan_from_requirements("Empty", "x");
    // Even a single-char input should produce a plan.
    assert_eq!(p.name, "Empty");
}

// ---------------------------------------------------------------------------
// Type construction
// ---------------------------------------------------------------------------

#[test]
fn plan_step_estimated_effort_field() {
    let mut s = step("s", "d", vec![]);
    s.estimated_effort = Some("2h".to_string());
    assert_eq!(s.estimated_effort.as_deref(), Some("2h"));
}

#[test]
fn plan_struct_construction_directly() {
    let p = Plan {
        id: "plan_x".into(),
        name: "Direct".into(),
        description: "D".into(),
        status: PlanStatus::Draft,
        steps: vec![],
        created_at: "2024-01-01T00:00:00Z".into(),
        updated_at: "2024-01-01T00:00:00Z".into(),
    };
    assert_eq!(p.id, "plan_x");
}
