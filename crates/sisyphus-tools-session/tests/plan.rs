//! Integration tests for sisyphus-tools-session `plan` module.
//!
//! Exercises the public surfaces:
//! - `plan_review` (pure function returning review comments)
//! - `plan_create` (constructor)
//! - `plan_approve` / `plan_cancel` (status transitions)

use sisyphus_tools_session::plan::{
    plan_approve, plan_cancel, plan_create, plan_review, Plan, PlanStatus, PlanStep,
};

// ---------------------------------------------------------------------------
// plan_review — pure function
// ---------------------------------------------------------------------------

#[test]
fn plan_review_empty_name_comments() {
    let p = plan_create("", "desc", vec![]);
    let c = plan_review(&p);
    assert!(c.iter().any(|s| s.contains("name is empty")));
}

#[test]
fn plan_review_empty_description_comments() {
    let p = plan_create("name", "", vec![]);
    let c = plan_review(&p);
    assert!(c.iter().any(|s| s.contains("description is empty")));
}

#[test]
fn plan_review_no_steps_comments() {
    let p = plan_create("name", "desc", vec![]);
    let c = plan_review(&p);
    assert!(c.iter().any(|s| s.contains("no steps")));
}

#[test]
fn plan_review_step_empty_description_comments() {
    let p = plan_create(
        "name",
        "desc",
        vec![PlanStep {
            id: "s1".into(),
            description: "".into(),
            status: PlanStatus::Draft,
            dependencies: vec![],
            estimated_effort: None,
        }],
    );
    let c = plan_review(&p);
    assert!(c.iter().any(|s| s.contains("empty description")));
}

#[test]
fn plan_review_step_depends_on_nonexistent() {
    let p = plan_create(
        "name",
        "desc",
        vec![PlanStep {
            id: "s1".into(),
            description: "d".into(),
            status: PlanStatus::Draft,
            dependencies: vec!["does_not_exist".into()],
            estimated_effort: None,
        }],
    );
    let c = plan_review(&p);
    assert!(c.iter().any(|s| s.contains("does not exist")));
}

#[test]
fn plan_review_clean_plan_no_comments() {
    let p = plan_create(
        "My Plan",
        "Build a thing",
        vec![
            PlanStep {
                id: "s1".into(),
                description: "first".into(),
                status: PlanStatus::Draft,
                dependencies: vec![],
                estimated_effort: Some("1h".into()),
            },
            PlanStep {
                id: "s2".into(),
                description: "second".into(),
                status: PlanStatus::Draft,
                dependencies: vec!["s1".into()],
                estimated_effort: Some("2h".into()),
            },
        ],
    );
    let c = plan_review(&p);
    assert!(c.is_empty(), "expected no comments, got {c:?}");
}

// ---------------------------------------------------------------------------
// plan_create
// ---------------------------------------------------------------------------

#[test]
fn plan_create_assigns_id() {
    let p = plan_create("My Plan", "Build a thing", vec![]);
    assert!(!p.id.is_empty());
    assert_eq!(p.name, "My Plan");
    assert_eq!(p.description, "Build a thing");
    assert_eq!(p.status, PlanStatus::Draft);
    assert!(p.steps.is_empty());
}

#[test]
fn plan_create_with_steps() {
    let p = plan_create(
        "n",
        "d",
        vec![PlanStep {
            id: "s1".into(),
            description: "first".into(),
            status: PlanStatus::Draft,
            dependencies: vec![],
            estimated_effort: None,
        }],
    );
    assert_eq!(p.steps.len(), 1);
    assert_eq!(p.steps[0].id, "s1");
}

#[test]
fn plan_create_id_is_nonempty() {
    // The ID format is `plan_<hex>` where the hex is derived from
    // timestamp length. Just verify it's non-empty and starts with "plan_".
    let p = plan_create("a", "a", vec![]);
    assert!(!p.id.is_empty());
    assert!(p.id.starts_with("plan_"));
}

// ---------------------------------------------------------------------------
// plan_approve / plan_cancel
// ---------------------------------------------------------------------------

#[test]
fn plan_approve_changes_status() {
    let mut p = plan_create("n", "d", vec![]);
    plan_approve(&mut p);
    assert_eq!(p.status, PlanStatus::Approved);
}

#[test]
fn plan_cancel_changes_status() {
    let mut p = plan_create("n", "d", vec![]);
    plan_cancel(&mut p);
    assert_eq!(p.status, PlanStatus::Cancelled);
}

// ---------------------------------------------------------------------------
// PlanStep / PlanStatus serde
// ---------------------------------------------------------------------------

#[test]
fn plan_status_serde() {
    for s in [
        PlanStatus::Draft,
        PlanStatus::Reviewed,
        PlanStatus::Approved,
        PlanStatus::Cancelled,
    ] {
        let j = serde_json::to_string(&s).unwrap();
        let back: PlanStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(format!("{back:?}"), format!("{s:?}"));
    }
}

#[test]
fn plan_serde_round_trip() {
    let p = plan_create(
        "n",
        "d",
        vec![PlanStep {
            id: "s1".into(),
            description: "first".into(),
            status: PlanStatus::Draft,
            dependencies: vec![],
            estimated_effort: Some("1h".into()),
        }],
    );
    let j = serde_json::to_string(&p).unwrap();
    let back: Plan = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, p.id);
    assert_eq!(back.name, "n");
    assert_eq!(back.steps.len(), 1);
}
