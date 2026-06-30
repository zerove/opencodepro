//! Shared gRPC types and client for the Sisyphus Bridge protocol.
//!
//! This crate compiles `bridge.proto` once and re-exports all generated
//! types at the crate root for ergonomic access. Both the bridge server
//! (`sisyphus-bridge`) and CLI client (`sisyphus-cli`) depend on this crate
//! instead of compiling the proto independently.

pub mod proto {
    tonic::include_proto!("sisyphus.bridge");
}

// Re-export all generated types and sub-modules at the crate root
// so callers can write `use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridge`
// or `use sisyphus_bridge_api::ExecuteSopRequest` directly.
pub use proto::*;
