use tonic::service::Interceptor;
use tonic::{metadata::MetadataValue, Request, Status};

/// gRPC Bearer token authentication interceptor.
///
/// Checks the `authorization` metadata header against a configured token.
/// When `expected_token` is empty, all requests pass through (dev mode).
#[derive(Clone)]
pub struct AuthInterceptor {
    expected: String,
}

impl AuthInterceptor {
    /// New.
    pub fn new(token: String) -> Self {
        Self { expected: token }
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        if self.expected.is_empty() {
            return Ok(request);
        }
        let bearer = format!("Bearer {}", self.expected);
        let expected_val: MetadataValue<_> =
            bearer.parse().map_err(|_| Status::internal("invalid auth token format"))?;

        match request.metadata().get("authorization") {
            Some(val) if val == expected_val => Ok(request),
            _ => Err(Status::unauthenticated("invalid or missing auth token")),
        }
    }
}
