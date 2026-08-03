//! Core kernel implementation — the main `SisyphusKernel` methods.
//!
//! Extracted from `lib.rs` for readability. Re-exported via `pub use kernel::*`.
//! The impl is split across domain submodules; `pub use` re-exports here keep
//! the original module path stable for downstream crates.

mod cache;
mod construction;
mod execution;
mod feedback;
mod project;
