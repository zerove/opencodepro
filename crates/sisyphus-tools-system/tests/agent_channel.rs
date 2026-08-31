//! Integration tests for sisyphus-tools-system `agent_channel`.
//!
//! Exercises the public `AgentChannel` API: direct messaging
//! (`send` / `receive`), pub/sub (`subscribe` / `publish`),
//! and request/response with timeout.

use sisyphus_tools_system::agent_channel::AgentChannel;
use sisyphus_types::AgentMessage;
use std::time::Duration;

fn make_msg(from: &str, to: &str) -> AgentMessage {
    AgentMessage {
        from: from.into(),
        to: to.into(),
        kind: "request".into(),
        payload: serde_json::json!({"data": 1}),
        correlation_id: format!("corr-{from}-{to}"),
    }
}

// ---------------------------------------------------------------------------
// Direct messaging
// ---------------------------------------------------------------------------

#[test]
fn send_delivers_to_target_inbox() {
    let ch = AgentChannel::new();
    ch.send(make_msg("alice", "bob")).expect("send");
    let inbox = ch.receive("bob").expect("receive");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].from, "alice");
    assert_eq!(inbox[0].to, "bob");
}

#[test]
fn receive_drains_inbox() {
    let ch = AgentChannel::new();
    ch.send(make_msg("a", "b")).expect("send");
    let first = ch.receive("b").expect("receive");
    assert_eq!(first.len(), 1);
    // Second receive should be empty.
    let second = ch.receive("b").expect("receive");
    assert!(second.is_empty());
}

#[test]
fn receive_for_unknown_agent_is_empty() {
    let ch = AgentChannel::new();
    let inbox = ch.receive("ghost").expect("receive");
    assert!(inbox.is_empty());
}

#[test]
fn receive_for_agent_returns_only_their_messages() {
    let ch = AgentChannel::new();
    ch.send(make_msg("a", "b")).expect("send");
    ch.send(make_msg("a", "c")).expect("send");
    ch.send(make_msg("a", "b")).expect("send");
    assert_eq!(ch.receive("b").expect("").len(), 2);
    assert_eq!(ch.receive("c").expect("").len(), 1);
}

#[test]
fn default_constructor_works() {
    let ch = AgentChannel::default();
    assert!(ch.receive("nobody").expect("").is_empty());
}

// ---------------------------------------------------------------------------
// Pub/Sub
// ---------------------------------------------------------------------------

#[test]
fn publish_delivers_to_all_subscribers() {
    let ch = AgentChannel::new();
    ch.subscribe("agent_a", "news").expect("sub");
    ch.subscribe("agent_b", "news").expect("sub");
    // No background processing — subscriptions are visible immediately.
    ch.publish("news", make_msg("broadcaster", "news")).expect("pub");
    assert_eq!(ch.receive("agent_a").expect("").len(), 1);
    assert_eq!(ch.receive("agent_b").expect("").len(), 1);
}

#[test]
fn publish_to_topic_with_no_subscribers_is_noop() {
    let ch = AgentChannel::new();
    ch.publish("nobody_cares", make_msg("a", "b")).expect("pub");
    // No one received the message.
    assert!(ch.receive("a").expect("").is_empty());
    assert!(ch.receive("b").expect("").is_empty());
}

#[test]
fn subscribe_to_multiple_topics() {
    let ch = AgentChannel::new();
    ch.subscribe("agent", "topic_a").expect("sub");
    ch.subscribe("agent", "topic_b").expect("sub");
    ch.publish("topic_a", make_msg("src", "topic_a")).expect("pub");
    ch.publish("topic_b", make_msg("src", "topic_b")).expect("pub");
    let inbox = ch.receive("agent").expect("");
    assert_eq!(inbox.len(), 2);
}

// ---------------------------------------------------------------------------
// Request/Response with timeout
// ---------------------------------------------------------------------------

#[tokio::test]
async fn request_times_out_without_responder() {
    let ch = AgentChannel::new();
    let result = ch
        .request(
            "nowhere",
            serde_json::json!({"q": 1}),
            "corr-1",
            Duration::from_millis(50),
        )
        .await;
    assert!(result.is_err());
    let msg = result.err().unwrap().to_string();
    assert!(
        msg.contains("timed out") || msg.contains("Request timed out"),
        "got: {msg}"
    );
}

#[tokio::test]
async fn request_completes_when_response_sent() {
    let ch = AgentChannel::new();
    let ch_clone = ch.clone();
    // Spawn a responder.
    let handle = tokio::spawn(async move {
        let inbox = ch_clone.receive("target").expect("");
        assert_eq!(inbox.len(), 1);
        let response = AgentMessage {
            from: "target".into(),
            to: "self".into(),
            kind: "response".into(),
            payload: serde_json::json!({"answer": "world"}),
            correlation_id: inbox[0].correlation_id.clone(),
        };
        ch_clone.send(response).expect("");
    });

    let result = ch
        .request(
            "target",
            serde_json::json!({"q": "hello"}),
            "corr-x",
            Duration::from_secs(1),
        )
        .await;
    let response = result.expect("ok");
    assert_eq!(response.kind, "response");
    assert_eq!(response.correlation_id, "corr-x");
    handle.await.expect("join");
}
