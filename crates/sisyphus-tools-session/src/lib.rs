pub mod session;
pub mod skill;
pub mod plan;
pub mod todo;
pub mod precommit;

pub use session::{
    CompressStrategy, Message, Session, SessionCheckpoint,
    session_add_message, session_clear_checkpoint, session_compress, session_continue,
    session_create, session_delete, session_estimate_token_count, session_export,
    session_export_html, session_fork, session_get, session_import, session_list,
    session_load_checkpoint, session_rename, session_save_checkpoint, session_undo,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_construction() {
        let msg = Message {
            role: "user".into(),
            content: "hello".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            estimated_tokens: 10,
            is_compressed: false,
        };
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.estimated_tokens, 10);
        assert!(!msg.is_compressed);
    }

    #[test]
    fn test_compress_strategy_default() {
        let strategy = CompressStrategy::default();
        if let CompressStrategy::Threshold(limit) = strategy {
            assert_eq!(limit, 32000);
        } else {
            panic!("Expected Threshold strategy");
        }
    }
}

