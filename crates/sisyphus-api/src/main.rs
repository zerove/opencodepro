//! REST API server for Sisyphus cognitive engine.
//! Provides HTTP endpoints for state management, vector search, and memory operations.

use axum::{
    Router,
    routing::{delete, get, post},
};
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use utoipa_swagger_ui::SwaggerUi;

mod handlers;
mod models;

pub use models::*;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::Pool<sqlx::Sqlite>,
}

/// Create the API router
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health check
        .route("/health", get(handlers::health_check))
        // State endpoints
        .route("/api/v1/state/:key", get(handlers::get_state))
        .route("/api/v1/state/:key", post(handlers::set_state))
        .route("/api/v1/state/:key", delete(handlers::delete_state))
        .route("/api/v1/state", get(handlers::list_state))
        // Swagger UI
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", handlers::openapi()))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(state))
}

/// Start the API server
pub async fn start_server(bind_addr: &str, db_url: &str) -> anyhow::Result<()> {
    // Initialize database
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_url)
        .await?;

    // Run migrations
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );
        "#,
    )
    .execute(&pool)
    .await?;

    let state = AppState { pool };
    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("Sisyphus API server listening on {}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sisyphus_api=info,tower_http=info".into()),
        )
        .init();

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:sisyphus.db".to_string());

    start_server(&bind_addr, &db_url).await
}
