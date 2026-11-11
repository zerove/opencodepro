//! gRPC trace id 拦截器（T4.4）。
//!
//! 从 metadata 读取 `x-trace-id`，缺失或非法则生成，并写回 metadata，
//! 使 gRPC 入口与其子步骤（SOP / 工具 / LLM / 事件流）可按同一 id 串联查询。

use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::{Request, Status};

use sisyphus_types::trace::{generate_trace_id, sanitize_trace_id, TRACE_ID_HEADER};

/// gRPC trace id 拦截器。
#[derive(Clone, Default)]
pub struct TraceInterceptor;

impl TraceInterceptor {
    /// New.
    pub fn new() -> Self {
        Self
    }
}

impl Interceptor for TraceInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let trace_id = request
            .metadata()
            .get(TRACE_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(sanitize_trace_id)
            .unwrap_or_else(generate_trace_id);

        if let Ok(value) = MetadataValue::try_from(trace_id.as_str()) {
            request.metadata_mut().insert(TRACE_ID_HEADER, value);
        }
        Ok(request)
    }
}

/// 组合拦截器：先鉴权，再注入 trace id。
#[derive(Clone)]
pub struct AuthTraceInterceptor {
    auth: crate::auth::AuthInterceptor,
    trace: TraceInterceptor,
}

impl AuthTraceInterceptor {
    /// New.
    pub fn new(token: String) -> Self {
        Self {
            auth: crate::auth::AuthInterceptor::new(token),
            trace: TraceInterceptor::new(),
        }
    }
}

impl Interceptor for AuthTraceInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        let request = self.auth.call(request)?;
        self.trace.call(request)
    }
}

/// 从 gRPC 请求 metadata 中读取 trace id（缺失时返回空串）。
///
/// 供服务实现把 trace id 记录进当前 span 的 `trace_id` 字段。
pub fn trace_id_of<T>(request: &tonic::Request<T>) -> String {
    request
        .metadata()
        .get(TRACE_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::metadata::MetadataKey;

    #[test]
    fn trace_id_is_generated_when_metadata_absent() {
        let mut interceptor = TraceInterceptor::new();
        let request = interceptor.call(Request::new(())).expect("应放行");
        let trace_id = request
            .metadata()
            .get(TRACE_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            trace_id.starts_with("trace-"),
            "应生成 trace id: {trace_id}"
        );
    }

    #[test]
    fn valid_trace_id_is_propagated() {
        let mut interceptor = TraceInterceptor::new();
        let mut request = Request::new(());
        request.metadata_mut().insert(TRACE_ID_HEADER, "trace-fixed".parse().unwrap());
        let request = interceptor.call(request).expect("应放行");
        assert_eq!(
            request
                .metadata()
                .get(MetadataKey::from_bytes(TRACE_ID_HEADER.as_bytes()).unwrap())
                .and_then(|v| v.to_str().ok()),
            Some("trace-fixed")
        );
    }

    #[test]
    fn invalid_trace_id_is_replaced() {
        let mut interceptor = TraceInterceptor::new();
        let mut request = Request::new(());
        request.metadata_mut().insert(TRACE_ID_HEADER, "bad id".parse().unwrap());
        let request = interceptor.call(request).expect("应放行");
        let trace_id = request
            .metadata()
            .get(TRACE_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(trace_id.starts_with("trace-"), "非法值应被替换: {trace_id}");
        assert_ne!(trace_id, "bad id");
    }
}
