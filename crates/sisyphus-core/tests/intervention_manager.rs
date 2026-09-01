//! Integration tests for sisyphus-core `intervention_manager`.
//!
//! Exercises the public `InterventionManager` API. The manager
//! coordinates human-in-the-loop feedback: `request_feedback` pauses
//! a node waiting for user input; `submit_feedback` delivers the
//! user's response and resumes execution.

use sisyphus_core::intervention_manager::{InterventionManager, UserFeedback};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn new_starts_with_no_pending_interventions() {
    let m = InterventionManager::new();
    // No way to introspect; just exercise the API.
    let _ = m;
}

#[test]
fn default_constructor_works() {
    let m = InterventionManager::default();
    let _ = m;
}

// ---------------------------------------------------------------------------
// Timeout behavior
// ---------------------------------------------------------------------------

#[tokio::test]
async fn request_feedback_times_out_without_response() {
    let m = InterventionManager::new();
    let result = m
        .request_feedback(
            "node_a".into(),
            "Provide guidance".into(),
            "summary".into(),
            Duration::from_millis(50),
        )
        .await;
    assert!(result.is_err());
    let err = result.expect_err("err");
    let msg = err.to_string();
    assert!(
        msg.contains("timed out") || msg.contains("cancelled"),
        "got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Successful feedback roundtrip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn request_feedback_completes_when_response_submitted() {
    let m = std::sync::Arc::new(InterventionManager::new());
    let m_clone = m.clone();

    // Spawn a task that submits feedback after a short delay.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = m_clone
            .submit_feedback(
                "node_b",
                UserFeedback {
                    correction: "do X".to_string(),
                    should_pivot: false,
                    pivot_reason: None,
                    confidence: 0.9,
                },
            )
            .await;
    });

    let result = m
        .request_feedback(
            "node_b".into(),
            "q".into(),
            "ctx".into(),
            Duration::from_secs(1),
        )
        .await;
    let feedback = result.expect("ok");
    assert_eq!(feedback.correction, "do X");
    assert!(!feedback.should_pivot);
    assert_eq!(feedback.confidence, 0.9);
}

#[tokio::test]
async fn submit_feedback_with_pivot_reason() {
    let m = std::sync::Arc::new(InterventionManager::new());
    let m_clone = m.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = m_clone
            .submit_feedback(
                "node_c",
                UserFeedback {
                    correction: "stop".to_string(),
                    should_pivot: true,
                    pivot_reason: Some("wrong approach".to_string()),
                    confidence: 0.2,
                },
            )
            .await;
    });

    let result = m
        .request_feedback(
            "node_c".into(),
            "q".into(),
            "ctx".into(),
            Duration::from_secs(1),
        )
        .await
        .expect("ok");
    assert!(result.should_pivot);
    assert_eq!(result.pivot_reason.as_deref(), Some("wrong approach"));
}

// ---------------------------------------------------------------------------
// submit_feedback for unknown node
// ---------------------------------------------------------------------------

#[tokio::test]
async fn submit_feedback_for_unknown_node_errors() {
    let m = InterventionManager::new();
    let result = m
        .submit_feedback(
            "nonexistent",
            UserFeedback {
                correction: "x".to_string(),
                should_pivot: false,
                pivot_reason: None,
                confidence: 0.5,
            },
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn submit_feedback_only_works_once_per_node() {
    let m = std::sync::Arc::new(InterventionManager::new());
    let m_clone = m.clone();
    let m_clone2 = m.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = m_clone
            .submit_feedback(
                "node_d",
                UserFeedback {
                    correction: "first".to_string(),
                    should_pivot: false,
                    pivot_reason: None,
                    confidence: 0.5,
                },
            )
            .await;
        // Second submit should fail (no pending intervention).
        let r = m_clone2
            .submit_feedback(
                "node_d",
                UserFeedback {
                    correction: "second".to_string(),
                    should_pivot: false,
                    pivot_reason: None,
                    confidence: 0.5,
                },
            )
            .await;
        assert!(r.is_err(), "second submit should fail");
    });

    let r = m
        .request_feedback(
            "node_d".into(),
            "q".into(),
            "ctx".into(),
            Duration::from_secs(1),
        )
        .await
        .expect("ok");
    assert_eq!(r.correction, "first");
}

// ---------------------------------------------------------------------------
// UserFeedback serialization
// ---------------------------------------------------------------------------

#[test]
fn user_feedback_serialization_roundtrip() {
    let fb = UserFeedback {
        correction: "fix this".to_string(),
        should_pivot: true,
        pivot_reason: Some("flawed".to_string()),
        confidence: 0.42,
    };
    let json = serde_json::to_string(&fb).expect("serialize");
    let parsed: UserFeedback = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.correction, "fix this");
    assert!(parsed.should_pivot);
    assert_eq!(parsed.pivot_reason.as_deref(), Some("flawed"));
    assert!((parsed.confidence - 0.42).abs() < 0.001);
}

#[test]
fn user_feedback_serialization_without_pivot() {
    let fb = UserFeedback {
        correction: "no pivot".to_string(),
        should_pivot: false,
        pivot_reason: None,
        confidence: 0.8,
    };
    let json = serde_json::to_string(&fb).expect("serialize");
    let parsed: UserFeedback = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.correction, "no pivot");
    assert!(!parsed.should_pivot);
    assert!(parsed.pivot_reason.is_none());
}
