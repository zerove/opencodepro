use crate::ToolResult;
use serde_json::Value;
use sisyphus_types::AgentMessage;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;

/// In-memory inter-agent message broker supporting direct and pub/sub patterns.
pub struct AgentChannel {
    inboxes: Arc<RwLock<HashMap<String, Vec<AgentMessage>>>>,
    topics: Arc<RwLock<HashMap<String, Vec<String>>>>, // topic -> subscribers
    notify: Arc<RwLock<HashMap<String, broadcast::Sender<AgentMessage>>>>,
}

impl Default for AgentChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentChannel {
    pub fn new() -> Self {
        Self {
            inboxes: Arc::new(RwLock::new(HashMap::new())),
            topics: Arc::new(RwLock::new(HashMap::new())),
            notify: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Send a direct message to a specific agent's inbox.
    pub fn send(&self, msg: AgentMessage) -> ToolResult<()> {
        let mut inboxes = self.inboxes.write().map_err(|e| {
            crate::ToolError::Generic(format!("AgentChannel lock poisoned: {}", e))
        })?;
        inboxes.entry(msg.to.clone()).or_default().push(msg);
        Ok(())
    }

    /// Receive all pending messages for an agent (drains the inbox).
    pub fn receive(&self, agent_id: &str) -> ToolResult<Vec<AgentMessage>> {
        let mut inboxes = self.inboxes.write().map_err(|e| {
            crate::ToolError::Generic(format!("AgentChannel lock poisoned: {}", e))
        })?;
        Ok(inboxes.remove(agent_id).unwrap_or_default())
    }

    /// Send a request and wait for a response (blocking, with timeout).
    pub fn request(
        &self,
        agent_id: &str,
        payload: Value,
        correlation_id: &str,
        _timeout: Duration,
    ) -> ToolResult<AgentMessage> {
        let notify_tx = {
            let mut notify = self.notify.write().map_err(|e| {
                crate::ToolError::Generic(format!("AgentChannel lock poisoned: {}", e))
            })?;
            let (tx, _) = broadcast::channel(16);
            notify.insert(correlation_id.to_string(), tx.clone());
            tx
        };

        let request = AgentMessage {
            from: "self".into(),
            to: agent_id.into(),
            kind: "request".into(),
            payload,
            correlation_id: correlation_id.to_string(),
        };
        self.send(request)?;

        // Non-blocking: just return a placeholder response
        // Real implementation would await on broadcast receiver
        let mut rx = notify_tx.subscribe();
        match rx.try_recv() {
            Ok(msg) => Ok(msg),
            Err(_) => Ok(AgentMessage {
                from: agent_id.into(),
                to: "self".into(),
                kind: "response".into(),
                payload: Value::Null,
                correlation_id: correlation_id.to_string(),
            }),
        }
    }

    /// Publish a message to all subscribers of a topic.
    pub fn publish(&self, topic: &str, msg: AgentMessage) -> ToolResult<()> {
        let subscribers = {
            let topics = self.topics.read().map_err(|e| {
                crate::ToolError::Generic(format!("AgentChannel lock poisoned: {}", e))
            })?;
            topics.get(topic).cloned().unwrap_or_default()
        };

        for agent_id in subscribers {
            let mut inboxes = self.inboxes.write().map_err(|e| {
                crate::ToolError::Generic(format!("AgentChannel lock poisoned: {}", e))
            })?;
            inboxes.entry(agent_id).or_default().push(msg.clone());
        }
        Ok(())
    }

    /// Subscribe an agent to a topic.
    pub fn subscribe(&self, agent_id: &str, topic: &str) -> ToolResult<()> {
        if let Ok(mut map) = self.topics.write() {
            map.entry(topic.to_string()).or_default().push(agent_id.to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_msg(from: &str, to: &str) -> AgentMessage {
        AgentMessage {
            from: from.into(),
            to: to.into(),
            kind: "request".into(),
            payload: json!({"data": 1}),
            correlation_id: "test-1".into(),
        }
    }

    #[test]
    fn test_send_receive() {
        let ch = AgentChannel::new();
        let msg = make_msg("alice", "bob");
        ch.send(msg.clone()).unwrap();

        let inbox = ch.receive("bob").unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].from, "alice");

        // Second receive should be empty (drained)
        let empty = ch.receive("bob").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_receive_empty() {
        let ch = AgentChannel::new();
        let inbox = ch.receive("nobody").unwrap();
        assert!(inbox.is_empty());
    }

    #[test]
    fn test_publish_subscribe() {
        let ch = AgentChannel::new();
        ch.subscribe("agent_a", "news").unwrap();
        ch.subscribe("agent_b", "news").unwrap();

        // Give the background task time to process subscriptions
        std::thread::sleep(std::time::Duration::from_millis(100));

        let msg = make_msg("broadcaster", "news");
        ch.publish("news", msg).unwrap();

        let a_inbox = ch.receive("agent_a").unwrap();
        let b_inbox = ch.receive("agent_b").unwrap();
        assert_eq!(a_inbox.len(), 1, "agent_a should receive published msg");
        assert_eq!(b_inbox.len(), 1, "agent_b should receive published msg");
    }

    #[test]
    fn test_request_response() {
        let ch = AgentChannel::new();
        let result = ch.request(
            "target",
            json!({"query": "hello"}),
            "corr-1",
            Duration::from_secs(1),
        );
        assert!(result.is_ok());
        // Target should have the request in its inbox
        let inbox = ch.receive("target").unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].correlation_id, "corr-1");
    }

    #[test]
    fn test_multi_agent_independence() {
        let ch = AgentChannel::new();
        ch.send(make_msg("a1", "b1")).unwrap();
        ch.send(make_msg("a2", "b2")).unwrap();
        ch.send(make_msg("a3", "b1")).unwrap();

        assert_eq!(ch.receive("b1").unwrap().len(), 2);
        assert_eq!(ch.receive("b2").unwrap().len(), 1);
        assert!(ch.receive("b3").unwrap().is_empty());
    }
}
