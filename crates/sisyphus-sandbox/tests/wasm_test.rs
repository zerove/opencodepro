use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::ToolExecutor;

#[tokio::test]
async fn test_hello_wasm_tool() {
    let executor = WasmToolExecutor::new();
    let result = executor
        .execute("hello", vec![], 1000)
        .await
        .expect("Execution failed");

    let result_str = String::from_utf8(result).expect("Invalid UTF-8 result");
    assert_eq!(result_str, "Hello from WASM!");
}

#[tokio::test]
async fn test_fuel_exhaustion() {
    let executor = WasmToolExecutor::new();
    let result = executor.execute("infinite_loop", vec![], 100).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("ResourceExhausted"));
}

#[tokio::test]
async fn test_memory_exhaustion() {
    let executor = WasmToolExecutor::new();
    let result = executor.execute("memory_hog", vec![], 1000).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    // memory_hog tries memory.grow 1000, blocked by max_memory_size(10*65536)
    assert!(
        err.contains("ResourceExhausted") || err.contains("memory") || err.contains("translation"),
        "Expected memory-related error, got: {}",
        err
    );
}
