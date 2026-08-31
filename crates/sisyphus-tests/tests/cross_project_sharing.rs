//! Cross-Project Experience Sharing Integration Tests
//!
//! Verifies the multi-workspace knowledge transfer mechanism:
//! - Experiences are isolated per project by default
//! - `share_experience` makes an experience globally visible to all projects
//! - `unshare_experience` restores project ownership
//! - Shared experiences appear in the shared listing

use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::{SisyphusStore, VectorMemoryStore};
use sisyphus_tools_llm::DummyEmbeddingClient;
use sisyphus_types::experience::{ExperienceBank, ExperienceQuery, ExperienceTuple};
use sisyphus_types::TypesError;
use std::sync::Arc;

/// Build a real VectorExperienceBank over an in-memory database.
async fn setup_experience_bank() -> Arc<VectorExperienceBank> {
    let store = SisyphusStore::new("sqlite::memory:", &sisyphus_types::Config::default())
        .await
        .expect("Failed to init SisyphusStore");
    let state_store = store.state.clone();
    let vector_store =
        Arc::new(VectorMemoryStore::new(store.pool()).await.expect("Vector store failed"));

    Arc::new(
        VectorExperienceBank::new(state_store, vector_store, Arc::new(DummyEmbeddingClient))
            .await
            .expect("Exp bank init failed"),
    )
}

fn make_experience(context: &str, project_id: Option<&str>, timestamp: u64) -> ExperienceTuple {
    ExperienceTuple {
        context_text: context.to_string(),
        context_hash: format!("hash-{timestamp}"),
        action_id: "retry".to_string(),
        project_id: project_id.map(|p| p.to_string()),
        outcome_success: true,
        deviation_score: 0.1,
        cost: 10,
        timestamp,
        embedding: None,
        visual_embedding: None,
        insight: None,
    }
}

async fn query_project(
    bank: &dyn ExperienceBank,
    text: &str,
    project_id: &str,
) -> Vec<ExperienceTuple> {
    bank.query_similar(ExperienceQuery {
        query_text: text.to_string(),
        context_hash: "query".to_string(),
        project_id: Some(project_id.to_string()),
        action_id: None,
        visual_embedding: None,
        limit: 10,
    })
    .await
    .expect("query failed")
}

#[tokio::test]
async fn test_experience_isolated_between_projects() {
    let bank = setup_experience_bank().await;

    bank.record(make_experience(
        "project-a-only lesson",
        Some("proj-a"),
        1001,
    ))
    .await
    .expect("record failed");

    // Project B must NOT see project A's private experience.
    let results_b = query_project(bank.as_ref(), "project-a-only lesson", "proj-b").await;
    assert!(
        results_b.is_empty(),
        "project B saw project A's private experience"
    );

    // Project A itself must see it.
    let results_a = query_project(bank.as_ref(), "project-a-only lesson", "proj-a").await;
    assert_eq!(results_a.len(), 1, "project A lost its own experience");
}

#[tokio::test]
async fn test_share_makes_experience_visible_across_projects() {
    let bank = setup_experience_bank().await;

    bank.record(make_experience(
        "shared retry strategy",
        Some("proj-a"),
        2001,
    ))
    .await
    .expect("record failed");

    // Before sharing: invisible to project B.
    let before = query_project(bank.as_ref(), "shared retry strategy", "proj-b").await;
    assert!(
        before.is_empty(),
        "experience should be private before sharing"
    );

    // Share it globally.
    bank.share_experience("exp:2001").await.expect("share failed");

    // After sharing: project B can now retrieve it (multi-workspace transfer).
    let after = query_project(bank.as_ref(), "shared retry strategy", "proj-b").await;
    assert!(
        after.iter().any(|e| e.timestamp == 2001),
        "shared experience not visible to project B"
    );

    // Shared listing contains it.
    let shared = bank.list_shared().await.expect("list_shared failed");
    assert!(
        shared.iter().any(|e| e.timestamp == 2001 && e.project_id.is_none()),
        "shared experience missing from shared listing"
    );

    // Project A still sees it.
    let own = query_project(bank.as_ref(), "shared retry strategy", "proj-a").await;
    assert!(own.iter().any(|e| e.timestamp == 2001));
}

#[tokio::test]
async fn test_unshare_restores_project_ownership() {
    let bank = setup_experience_bank().await;

    bank.record(make_experience("rollback pattern", Some("proj-a"), 3001))
        .await
        .expect("record failed");

    bank.share_experience("exp:3001").await.expect("share failed");
    bank.unshare_experience("exp:3001", "proj-a").await.expect("unshare failed");

    // No longer shared.
    let shared = bank.list_shared().await.expect("list_shared failed");
    assert!(
        shared.iter().all(|e| e.timestamp != 3001),
        "unshared experience still listed as shared"
    );

    // Back to project A only.
    let b = query_project(bank.as_ref(), "rollback pattern", "proj-b").await;
    assert!(
        b.iter().all(|e| e.timestamp != 3001),
        "unshared experience visible to project B"
    );
    let a = query_project(bank.as_ref(), "rollback pattern", "proj-a").await;
    assert!(a.iter().any(|e| e.timestamp == 3001));
}

#[tokio::test]
async fn test_share_missing_experience_errors() {
    let bank = setup_experience_bank().await;

    let err = bank.share_experience("exp:999999").await.unwrap_err();
    assert!(
        matches!(err, TypesError::ExperienceError(_)),
        "expected ExperienceError, got {err:?}"
    );
}
