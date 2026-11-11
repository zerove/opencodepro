//! Trace 关联（T4.4）：请求级标识的生成与校验。
//!
//! 入口（HTTP / gRPC）生成或透传 trace id，该 id 随响应头、事件负载与日志
//! 传播，使「一次请求」与其触发的全部子步骤可被串联查询。
//!
//! 约定：
//! - 头名统一为 [`TRACE_ID_HEADER`]（HTTP 头 / gRPC metadata 共用）；
//! - trace id 仅允许 `[A-Za-z0-9_-]`，长度上限 [`TRACE_ID_MAX_LEN`]，
//!   非法输入一律丢弃并重新生成（避免日志伪造与注入）。

/// HTTP 头 / gRPC metadata 中承载 trace id 的键名。
pub const TRACE_ID_HEADER: &str = "x-trace-id";

/// 允许透传的 trace id 最大长度。
pub const TRACE_ID_MAX_LEN: usize = 64;

/// 生成一个新的 trace id（`trace-` + 32 位十六进制）。
pub fn generate_trace_id() -> String {
    format!("trace-{}", uuid::Uuid::new_v4().simple())
}

/// 校验并规范化外部传入的 trace id。
///
/// 返回 `None` 表示输入非法（空、过长或含非白名单字符），调用方应重新生成。
pub fn sanitize_trace_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > TRACE_ID_MAX_LEN {
        return None;
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    Some(trimmed.to_string())
}

/// 从候选值中解析出可用的 trace id：优先透传合法值，否则生成新的。
pub fn resolve_trace_id(candidate: Option<&str>) -> String {
    candidate.and_then(sanitize_trace_id).unwrap_or_else(generate_trace_id)
}

/// 请求上下文中的 trace id（由入口中间件注入，供 handler / 子步骤读取）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceId(pub String);

impl TraceId {
    /// 取 trace id 字符串。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_trace_id_has_expected_shape() {
        let id = generate_trace_id();
        assert!(id.starts_with("trace-"));
        assert_eq!(id.len(), "trace-".len() + 32);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn generated_trace_ids_are_distinct() {
        assert_ne!(generate_trace_id(), generate_trace_id());
    }

    #[test]
    fn sanitize_accepts_valid_ids() {
        assert_eq!(
            sanitize_trace_id("trace-abc123"),
            Some("trace-abc123".to_string())
        );
        assert_eq!(
            sanitize_trace_id("  trace-abc  "),
            Some("trace-abc".to_string())
        );
    }

    #[test]
    fn sanitize_rejects_invalid_ids() {
        assert!(sanitize_trace_id("").is_none());
        assert!(sanitize_trace_id("   ").is_none());
        assert!(sanitize_trace_id("bad id with space").is_none());
        assert!(sanitize_trace_id("bad\nnewline").is_none());
        assert!(sanitize_trace_id("bad;semicolon").is_none());
        assert!(sanitize_trace_id(&"x".repeat(TRACE_ID_MAX_LEN + 1)).is_none());
    }

    #[test]
    fn resolve_prefers_valid_candidate() {
        assert_eq!(
            resolve_trace_id(Some("trace-keep")),
            "trace-keep".to_string()
        );
    }

    #[test]
    fn resolve_replaces_invalid_candidate() {
        let resolved = resolve_trace_id(Some("bad id"));
        assert!(resolved.starts_with("trace-"));
        assert_ne!(resolved, "bad id");
    }
}
