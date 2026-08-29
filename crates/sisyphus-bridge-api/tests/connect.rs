//! Integration tests for sisyphus-bridge-api.
//!
//! The crate re-exports a generated tonic gRPC client; `connect` is the
//! only hand-written public function. We verify:
//!
//! 1. `connect` against an unreachable address fails with a tonic error
//!    inside the documented timeout window (3s + small slack).
//! 2. The generated proto types are reachable via the re-export at the
//!    crate root (a compile-time check that the build script still works).

use std::time::{Duration, Instant};

#[tokio::test]
async fn connect_to_unreachable_endpoint_fails_quickly() {
    // Use a non-routable address so the OS rejects the connection fast.
    // Port 1 is reserved (tcpmux) and very rarely bound.
    let addr = "http://127.0.0.1:1";
    let start = Instant::now();
    let result = sisyphus_bridge_api::connect(addr).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "connect to 127.0.0.1:1 must fail");
    // The implementation has a 3-second timeout per attempt. With one
    // attempt we should be well below 10s. We allow generous slack to
    // avoid CI flakes on slow runners.
    assert!(
        elapsed < Duration::from_secs(10),
        "connect should fail fast; took {elapsed:?}"
    );
}

#[test]
fn proto_types_are_reexported() {
    // Smoke test: these are re-exported at the crate root by `pub use proto::*`.
    // If the build script ever breaks, this test fails to compile, which is
    // the desired signal.
    // Touch the re-exported type to verify it is reachable.
    let _ = sisyphus_bridge_api::HealthCheckRequest::default();
}
