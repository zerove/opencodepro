//! Server-Sent Events (SSE) handlers for real-time event streaming.
//!
//! Provides `/api/events` endpoint for real-time event streaming using Server-Sent Events.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::convert::Infallible;
use std::time::Duration;
use uuid::Uuid;

use crate::http::RestApiState;

/// SSE event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    /// Cognitive iteration started
    CognitiveIterationStarted {
        iteration: usize,
        goal_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Cognitive iteration completed
    CognitiveIterationCompleted {
        iteration: usize,
        goal_id: String,
        deviation: f64,
        converged: bool,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Node execution started
    NodeExecutionStarted {
        sop_id: String,
        node_id: String,
        node_name: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Node execution completed
    NodeExecutionCompleted {
        sop_id: String,
        node_id: String,
        node_name: String,
        success: bool,
        output: Option<serde_json::Value>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// SOP execution started
    SopExecutionStarted {
        sop_id: String,
        goal_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// SOP execution completed
    SopExecutionCompleted {
        sop_id: String,
        goal_id: String,
        success: bool,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Chaos fault injected
    ChaosFaultInjected {
        fault_type: String,
        target: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Chaos auto-fix triggered
    ChaosAutoFixTriggered {
        fault_count: u64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Skill installed
    SkillInstalled {
        skill_id: String,
        version: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Skill uninstalled
    SkillUninstalled {
        skill_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Plugin loaded
    PluginLoaded {
        plugin_id: String,
        version: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Plugin unloaded
    PluginUnloaded {
        plugin_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Workspace switched
    WorkspaceSwitched {
        workspace_id: String,
        workspace_name: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Session message added
    SessionMessageAdded {
        session_id: String,
        role: String,
        content: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Custom event
    Custom {
        event_type: String,
        payload: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

impl SseEvent {
    /// Create a custom event
    #[allow(dead_code)] // Public API for broadcasting custom events
    pub fn custom(event_type: String, payload: serde_json::Value) -> Self {
        Self::Custom {
            event_type,
            payload,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Get the event type as a string
    pub fn event_type(&self) -> Cow<'static, str> {
        match self {
            SseEvent::CognitiveIterationStarted { .. } => "cognitive_iteration_started".into(),
            SseEvent::CognitiveIterationCompleted { .. } => "cognitive_iteration_completed".into(),
            SseEvent::NodeExecutionStarted { .. } => "node_execution_started".into(),
            SseEvent::NodeExecutionCompleted { .. } => "node_execution_completed".into(),
            SseEvent::SopExecutionStarted { .. } => "sop_execution_started".into(),
            SseEvent::SopExecutionCompleted { .. } => "sop_execution_completed".into(),
            SseEvent::ChaosFaultInjected { .. } => "chaos_fault_injected".into(),
            SseEvent::ChaosAutoFixTriggered { .. } => "chaos_auto_fix_triggered".into(),
            SseEvent::SkillInstalled { .. } => "skill_installed".into(),
            SseEvent::SkillUninstalled { .. } => "skill_uninstalled".into(),
            SseEvent::PluginLoaded { .. } => "plugin_loaded".into(),
            SseEvent::PluginUnloaded { .. } => "plugin_unloaded".into(),
            SseEvent::WorkspaceSwitched { .. } => "workspace_switched".into(),
            SseEvent::SessionMessageAdded { .. } => "session_message_added".into(),
            SseEvent::Custom { event_type, .. } => Cow::Owned(event_type.clone()),
        }
    }

    /// Convert to SSE format string
    #[allow(dead_code)] // Used by tests and external consumers
    pub fn to_sse_string(&self) -> String {
        let data = serde_json::to_string(self).unwrap_or_default();
        format!("event: {}\ndata: {}\n\n", self.event_type(), data)
    }
}

/// SSE connection query parameters
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Query params parsed from URL, used for filtering
pub struct SseQuery {
    /// Filter by event types (comma-separated)
    pub events: Option<String>,
    /// Filter by project ID
    pub project_id: Option<String>,
    /// Filter by SOP ID
    pub sop_id: Option<String>,
    /// Filter by session ID
    pub session_id: Option<String>,
    /// Last event ID for resumption
    pub last_event_id: Option<String>,
    /// Heartbeat interval in seconds (0 = disabled)
    pub heartbeat: Option<u64>,
}

/// SSE 并发连接数限制失败时的 HTTP 状态码
const SSE_LIMIT_REACHED: StatusCode = StatusCode::TOO_MANY_REQUESTS;

/// SSE stream handler
pub async fn sse_handler(
    State(state): State<RestApiState>,
    Query(query): Query<SseQuery>,
) -> Response {
    // 并发连接数限制：超过上限直接返回 429，避免无限连接耗尽资源
    let _permit = match state.sse_connection_limit.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                SSE_LIMIT_REACHED,
                Json(json!({"error": "Too many SSE connections"})),
            )
                .into_response();
        }
    };

    // 订阅全局广播器（所有客户端共享同一 channel）
    let receiver = state.sse_broadcaster.subscribe();

    // Parse event filters
    let event_filters: Option<Vec<String>> = query
        .events
        .as_ref()
        .map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

    let heartbeat_interval = query.heartbeat.unwrap_or(30);

    // Create event stream from broadcast receiver
    let stream = async_stream::stream! {
        let mut receiver = receiver;
        let mut last_heartbeat = std::time::Instant::now();

        loop {
            // Check if it's time for a heartbeat
            let now = std::time::Instant::now();
            if heartbeat_interval > 0 && now.duration_since(last_heartbeat) >= Duration::from_secs(heartbeat_interval) {
                last_heartbeat = now;
                yield Ok::<axum::response::sse::Event, Infallible>(
                    axum::response::sse::Event::default()
                        .event("heartbeat")
                        .data(json!({"timestamp": chrono::Utc::now().to_rfc3339()}).to_string()),
                );
            }

            // Try to receive an event with a timeout
            match tokio::time::timeout(
                Duration::from_millis(100),
                receiver.recv()
            ).await {
                Ok(Ok(event)) => {
                    // Apply filters
                    if let Some(ref filters) = event_filters {
                        if !filters.contains(&event.event_type().to_string()) {
                            continue;
                        }
                    }

                    let sse_event = axum::response::sse::Event::default()
                        .event(event.event_type())
                        .data(serde_json::to_string(&event).unwrap_or_default())
                        .id(Uuid::new_v4().to_string());

                    yield Ok::<axum::response::sse::Event, Infallible>(sse_event);
                }
                Ok(Err(_)) => break,
                Err(_) => {
                    // Timeout - continue loop for heartbeat
                    continue;
                }
            }
        }
    };

    Sse::new(Box::pin(stream))
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(Duration::from_secs(heartbeat_interval))
                .text("keep-alive"),
        )
        .into_response()
}

/// REST endpoint to broadcast an event (for internal use)
pub async fn broadcast_event_handler(
    State(state): State<RestApiState>,
    Json(payload): Json<BroadcastEventRequest>,
) -> impl IntoResponse {
    let event = SseEvent::Custom {
        event_type: payload.event_type.clone(),
        payload: payload.payload,
        timestamp: chrono::Utc::now(),
    };

    match state.sse_broadcaster.send(event) {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "broadcasted"}))).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "No subscribers"})),
        )
            .into_response(),
    }
}

/// Request to broadcast an event
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Fields parsed from request body
pub struct BroadcastEventRequest {
    pub event_type: String,
    pub payload: serde_json::Value,
    pub project_id: Option<String>,
    pub sop_id: Option<String>,
    pub session_id: Option<String>,
}

/// Health check for SSE endpoint
pub async fn sse_health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "service": "sse"})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_sse_event_serialization() {
        let event = SseEvent::CognitiveIterationStarted {
            iteration: 1,
            goal_id: "goal1".to_string(),
            timestamp: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("cognitive_iteration_started"));
        assert!(json.contains("goal1"));
    }

    #[test]
    fn test_sse_event_type() {
        let event = SseEvent::CognitiveIterationStarted {
            iteration: 1,
            goal_id: "goal1".to_string(),
            timestamp: chrono::Utc::now(),
        };

        assert_eq!(event.event_type(), "cognitive_iteration_started");
    }

    #[test]
    fn test_sse_event_to_string() {
        let event = SseEvent::Custom {
            event_type: "test_event".to_string(),
            payload: json!({"key": "value"}),
            timestamp: chrono::Utc::now(),
        };

        let sse_str = event.to_sse_string();
        assert!(sse_str.contains("event: test_event"));
        assert!(sse_str.contains("data:"));
    }

    #[tokio::test]
    async fn test_sse_broadcast_shared_channel() {
        // 同一 channel 内：先订阅后广播，订阅端必须收到事件
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let event = SseEvent::Custom {
            event_type: "shared_test".to_string(),
            payload: json!({"k": "v"}),
            timestamp: chrono::Utc::now(),
        };
        assert!(tx.send(event.clone()).is_ok());
        let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("同一 channel 内应能收到广播事件")
            .expect("发送不应失败");
        assert_eq!(received.event_type(), "shared_test");
    }

    #[tokio::test]
    async fn test_sse_connection_limit() {
        // 直接验证信号量限制：两个 acquire 成功，但当达到上限后应失败
        let semaphore = Arc::new(tokio::sync::Semaphore::new(2));
        let p1 = semaphore.clone().try_acquire_owned();
        let p2 = semaphore.clone().try_acquire_owned();
        assert!(p1.is_ok());
        assert!(p2.is_ok());
        // 已用尽 2 个许可，第 3 个获取失败
        let p3 = semaphore.clone().try_acquire_owned();
        assert!(p3.is_err(), "超过上限的获取应失败");
        drop(p1);
        drop(p2);
        // 释放后可再次获取
        let p4 = semaphore.clone().try_acquire_owned();
        assert!(p4.is_ok());
        drop(p4);
    }
}
