//! Sisyphus Rust SDK
//!
//! Client library for interacting with the Sisyphus cognitive engine REST API.
//!
//! # Example
//! ```rust,no_run
//! use sisyphus_sdk::SisyphusClient;
//!
//! #[tokio::main]
//! async fn main() {
//!     let client = SisyphusClient::new("http://localhost:3000");
//!     
//!     // Health check
//!     let health = client.health().await.unwrap();
//!     println!("Status: {}", health.status);
//!     
//!     // Set state
//!     client.set_state("my-key", serde_json::json!({"hello": "world"})).await.unwrap();
//!     
//!     // Get state
//!     let state = client.get_state("my-key").await.unwrap();
//!     println!("Value: {}", state.value);
//! }
//! ```

pub mod client;
pub mod error;
pub mod models;

pub use client::SisyphusClient;
pub use error::SdkError;
pub use models::*;
