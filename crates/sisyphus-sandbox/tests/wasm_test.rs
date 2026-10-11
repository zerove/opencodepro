use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::ToolExecutor;
use std::path::PathBuf;

fn test_executor() -> WasmToolExecutor {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tools_path = manifest_dir.parent().unwrap().parent().unwrap().join("tools");
    WasmToolExecutor::with_config(Some(tools_path), None).unwrap()
}

#[tokio::test]
async fn test_hello_wasm_tool() {
    let executor = test_executor();
    let result = executor.execute("hello", vec![], 1000).await.expect("Execution failed");

    let result_str = String::from_utf8(result).expect("Invalid UTF-8 result");
    assert_eq!(result_str, "Hello from WASM!");
}

/// The fuel limit must stop the guest, and say so.
///
/// The `ResourceExhausted` token is asserted deliberately, not incidentally:
/// `sisyphus_intel::reflect::ErrorCategory::classify` scans error text for that
/// exact substring to decide whether a failed node is worth evolving around. If
/// a fuel trap stopped carrying it, fuel exhaustion would silently be filed as
/// an ordinary failure and the reflection layer would lose its signal.
#[tokio::test]
async fn test_fuel_exhaustion_is_reported_as_a_resource_limit() {
    let executor = test_executor();
    let err = executor
        .execute("infinite_loop", vec![], 100)
        .await
        .expect_err("an unbounded loop must not survive a 100-fuel budget");

    let text = err.to_string();
    assert!(
        text.contains("ResourceExhausted"),
        "a fuel trap must stay classified as a resource limit, got: {text}"
    );
    assert!(
        text.contains("fuel exhausted"),
        "the reason must name the limit that fired, got: {text}"
    );
    assert!(
        text.contains("infinite_loop"),
        "the error must name the tool that failed, got: {text}"
    );
}

/// A guest that traps on its own must NOT be reported as a resource limit.
///
/// This is the negative control for the test above. `infinite_loop` and this
/// module both stop execution, but one is the sandbox working and the other is a
/// broken guest — and the reflection layer treats them very differently
/// (`ResourceExhausted` is a tunable signal; an ordinary failure is not).
///
/// Before trap classification existed, *every* trap was wrapped in an error whose
/// text began `ResourceExhausted: `, so this test would have failed: a guest
/// `unreachable` was indistinguishable from a fuel limit, and evolvers were told
/// a buggy module had hit a resource ceiling.
#[tokio::test]
async fn test_guest_trap_is_not_mislabelled_as_a_resource_limit() {
    let executor = test_executor();
    let err = executor
        .execute("trapping_guest", vec![], 10_000_000)
        .await
        .expect_err("a guest that executes `unreachable` must not report success");

    let text = err.to_string();
    assert!(
        !text.contains("ResourceExhausted"),
        "a guest fault must not be labelled a resource limit, got: {text}"
    );
    assert!(
        text.contains("WASM execution failed"),
        "a guest fault must be reported as an execution failure, got: {text}"
    );
}
