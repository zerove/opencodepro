//! Axum authentication middleware for REST API
//!
//! Provides Bearer token extraction, validation, and RBAC authorization.

use axum::{
    async_trait,
    body::Body,
    extract::{FromRequestParts, Request},
    http::{header, request::Parts, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension,
};
use std::sync::Arc;

use crate::auth_jwt::Claims;

/// Authentication error types
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Missing Authorization header")]
    MissingHeader,
    #[error("Invalid Authorization header format")]
    InvalidFormat,
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    #[error("Token expired")]
    Expired,
    #[error("Insufficient permissions")]
    InsufficientPermissions,
    #[error("Token revoked")]
    Revoked,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::MissingHeader => (
                StatusCode::UNAUTHORIZED,
                "Missing Authorization header".to_string(),
            ),
            AuthError::InvalidFormat => (
                StatusCode::UNAUTHORIZED,
                "Invalid Authorization header format".to_string(),
            ),
            AuthError::InvalidToken(msg) => {
                (StatusCode::UNAUTHORIZED, format!("Invalid token: {}", msg))
            }
            AuthError::Expired => (StatusCode::UNAUTHORIZED, "Token expired".to_string()),
            AuthError::InsufficientPermissions => (
                StatusCode::FORBIDDEN,
                "Insufficient permissions".to_string(),
            ),
            AuthError::Revoked => (StatusCode::UNAUTHORIZED, "Token revoked".to_string()),
        };

        let body = serde_json::json!({
            "error": message,
            "code": status.as_u16()
        });

        (status, axum::Json(body)).into_response()
    }
}

/// Authenticated user extracted from token
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub claims: Claims,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Extract JWT service from extensions
        let jwt_service = parts
            .extensions
            .get::<Arc<crate::auth_jwt::JwtService>>()
            .ok_or(AuthError::InvalidToken(
                "JWT service not configured".to_string(),
            ))?;

        // Extract Authorization header
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError::MissingHeader)?;

        let token =
            crate::auth_jwt::extract_bearer_token(auth_header).ok_or(AuthError::InvalidFormat)?;

        // Validate access token
        let token_data = jwt_service
            .validate_access_token(&token)
            .map_err(|e| AuthError::InvalidToken(e.to_string()))?;

        Ok(AuthenticatedUser {
            claims: token_data.claims,
        })
    }
}

/// Optional authenticated user (allows unauthenticated access)
#[derive(Debug, Clone)]
pub struct OptionalAuthUser(pub Option<Claims>);

#[async_trait]
impl<S> FromRequestParts<S> for OptionalAuthUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let jwt_service = parts
            .extensions
            .get::<Arc<crate::auth_jwt::JwtService>>()
            .ok_or(AuthError::InvalidToken(
                "JWT service not configured".to_string(),
            ))?;

        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());

        let claims = if let Some(auth_header) = auth_header {
            if let Some(token) = crate::auth_jwt::extract_bearer_token(auth_header) {
                match jwt_service.validate_access_token(&token) {
                    Ok(token_data) => Some(token_data.claims),
                    Err(_) => None, // Invalid token treated as unauthenticated
                }
            } else {
                None
            }
        } else {
            None
        };

        Ok(OptionalAuthUser(claims))
    }
}

/// Require specific role for access
pub fn require_role(
    required_role: &'static str,
) -> impl Fn(AuthenticatedUser) -> Result<AuthenticatedUser, AuthError> + Clone {
    move |user: AuthenticatedUser| {
        if has_role(&user.claims, required_role) {
            Ok(user)
        } else {
            Err(AuthError::InsufficientPermissions)
        }
    }
}

/// Require access to specific project
pub fn require_project_access(
    project_id: String,
) -> impl Fn(AuthenticatedUser) -> Result<AuthenticatedUser, AuthError> + Clone {
    move |user: AuthenticatedUser| {
        if has_project_access(&user.claims, &project_id) {
            Ok(user)
        } else {
            Err(AuthError::InsufficientPermissions)
        }
    }
}

fn has_role(claims: &Claims, required_role: &str) -> bool {
    claims.roles.contains(&required_role.to_string()) || claims.roles.contains(&"admin".to_string())
}

fn has_project_access(claims: &Claims, project_id: &str) -> bool {
    claims.project_ids.contains(&project_id.to_string())
        || claims.project_ids.contains(&"*".to_string())
}

/// Authentication middleware for protected routes
pub async fn auth_middleware(
    Extension(_jwt_service): Extension<Arc<crate::auth_jwt::JwtService>>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, AuthError> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError::MissingHeader)?;

    let token =
        crate::auth_jwt::extract_bearer_token(auth_header).ok_or(AuthError::InvalidFormat)?;

    let jwt_service = request
        .extensions()
        .get::<Arc<crate::auth_jwt::JwtService>>()
        .ok_or(AuthError::InvalidToken(
            "JWT service not configured".to_string(),
        ))?;

    let token_data = jwt_service
        .validate_access_token(&token)
        .map_err(|e| AuthError::InvalidToken(e.to_string()))?;

    // Insert claims into request extensions for downstream handlers
    request.extensions_mut().insert(token_data.claims);

    Ok(next.run(request).await)
}

/// Optional auth middleware (allows unauthenticated access)
pub async fn optional_auth_middleware(
    Extension(jwt_service): Extension<Arc<crate::auth_jwt::JwtService>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    if let Some(auth_header) = auth_header {
        if let Some(token) = crate::auth_jwt::extract_bearer_token(auth_header) {
            if let Ok(token_data) = jwt_service.validate_access_token(&token) {
                request.extensions_mut().insert(token_data.claims);
            }
        }
    }

    next.run(request).await
}

/// Audit log entry
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditLogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<String>,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: uuid::Uuid,
}

/// Audit logging middleware
pub async fn audit_log_middleware(
    Extension(audit_logger): Extension<Arc<tokio::sync::Mutex<Vec<AuditLogEntry>>>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let start = std::time::Instant::now();
    let method = request.method().clone();
    let path_str = request.uri().path().to_string();
    let request_id = uuid::Uuid::new_v4();

    // Extract user ID from claims if present
    let user_id = request.extensions().get::<Claims>().map(|c| c.sub.clone());

    // Extract IP and User-Agent
    let ip_address = request
        .headers()
        .get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let user_agent = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let response = next.run(request).await;

    let status_code = response.status().as_u16();
    let duration = start.elapsed();

    let entry = AuditLogEntry {
        timestamp: chrono::Utc::now(),
        user_id,
        method: method.to_string(),
        path: path_str.clone(),
        status_code,
        ip_address,
        user_agent,
        request_id,
    };

    // Log to tracing
    tracing::info!(
        request_id = %request_id,
        method = %method,
        path = %path_str,
        status = status_code,
        duration_ms = duration.as_millis(),
        user_id = entry.user_id.as_deref().unwrap_or("anonymous"),
        "HTTP request completed"
    );

    // Store in audit log (in-memory for now, can be extended to persistent storage)
    audit_logger.lock().await.push(entry);

    response
}

/// Login request
#[derive(Debug, serde::Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String, // In production, use proper password hashing
}

/// Login response
#[derive(Debug, serde::Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub token_type: String,
}

/// Refresh token request
#[derive(Debug, serde::Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Logout request
#[derive(Debug, serde::Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

/// Auth handler for login
pub async fn login_handler(
    Extension(jwt_service): Extension<Arc<crate::auth_jwt::JwtService>>,
    axum::Json(req): axum::Json<LoginRequest>,
) -> Result<axum::Json<LoginResponse>, AuthError> {
    // In production, verify password against hashed storage
    // For now, accept any non-empty username/password for demo
    if req.username.is_empty() || req.password.is_empty() {
        return Err(AuthError::InvalidToken("Invalid credentials".to_string()));
    }

    // Determine roles based on username (demo logic)
    let roles = if req.username == "admin" {
        vec!["admin".to_string(), "user".to_string()]
    } else {
        vec!["user".to_string()]
    };

    // Determine project access (demo: user has access to their own project + wildcard for admin)
    let project_ids = if req.username == "admin" {
        vec!["*".to_string()]
    } else {
        vec![format!("proj_{}", req.username)]
    };

    let pair = jwt_service
        .create_token_pair(&req.username, roles, project_ids)
        .map_err(|e| AuthError::InvalidToken(e.to_string()))?;

    Ok(axum::Json(LoginResponse {
        access_token: pair.access_token,
        refresh_token: pair.refresh_token,
        expires_in: pair.expires_in,
        token_type: pair.token_type,
    }))
}

/// Auth handler for token refresh
pub async fn refresh_handler(
    Extension(jwt_service): Extension<Arc<crate::auth_jwt::JwtService>>,
    axum::Json(req): axum::Json<RefreshRequest>,
) -> Result<axum::Json<LoginResponse>, AuthError> {
    let pair = jwt_service
        .refresh_access_token(&req.refresh_token)
        .map_err(|e| AuthError::InvalidToken(e.to_string()))?;

    Ok(axum::Json(LoginResponse {
        access_token: pair.access_token,
        refresh_token: pair.refresh_token,
        expires_in: pair.expires_in,
        token_type: pair.token_type,
    }))
}

/// Auth handler for logout
pub async fn logout_handler(
    Extension(jwt_service): Extension<Arc<crate::auth_jwt::JwtService>>,
    axum::Json(req): axum::Json<LogoutRequest>,
) -> Result<axum::Json<serde_json::Value>, AuthError> {
    jwt_service
        .revoke_refresh_token(&req.refresh_token)
        .map_err(|e| AuthError::InvalidToken(e.to_string()))?;

    Ok(axum::Json(serde_json::json!({"success": true})))
}
