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

#[tokio::test]
async fn test_fuel_exhaustion() {
    let executor = test_executor();
    let result = executor.execute("infinite_loop", vec![], 100).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("ResourceExhausted"));
}

#[tokio::test]
async fn test_memory_exhaustion() {
    let executor = test_executor();
    let result = executor.execute("memory_hog", vec![], 1000).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    // memory_hog tries memory.grow 1000, blocked by max_memory_size(10*65536)
    let err_str = err.to_string();
    assert!(
        err_str.contains("ResourceExhausted")
            || err_str.contains("memory")
            || err_str.contains("translation"),
        "Expected memory-related error, got: {}",
        err_str
    );
}
