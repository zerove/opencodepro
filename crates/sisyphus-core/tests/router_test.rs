use sisyphus_tools_system::system_executor::SystemExecutor;
use sisyphus_types::{ToolExecutor, TypesError};
use std::sync::Arc;

struct DummyExecutor;
#[async_trait::async_trait]
impl ToolExecutor for DummyExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel: u64,
    ) -> Result<Vec<u8>, TypesError> {
        Ok(format!("dummy-exec-{}", tool_id).into_bytes())
    }
}

#[tokio::test]
async fn test_dynamic_tool_routing() {
    // 1. Setup Router with a default executor
    let default_exec = Arc::new(DummyExecutor);
    let router = sisyphus_types::ExecutorRouter::new(default_exec);
    let router_arc = Arc::new(router);

    // 2. Register SystemExecutor under 'sys' prefix
    let sys_exec = Arc::new(SystemExecutor);
    router_arc.register("sys", sys_exec);

    // 3. Test default routing (no prefix)
    let res = router_arc.execute("read_test", vec![], 0).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&res), "dummy-exec-read_test");

    // 4. Test prefixed routing (sys:read)
    // Create a temp file first
    let test_file = "test_router_verify.txt";
    std::fs::write(test_file, "router-works").unwrap();

    let res_sys = router_arc.execute("sys:read", test_file.as_bytes().to_vec(), 0).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&res_sys), "router-works");

    // 5. Test prefixed routing (sys:shell)
    let res_shell = router_arc.execute("sys:shell", b"echo hello".to_vec(), 0).await.unwrap();
    assert!(String::from_utf8_lossy(&res_shell).contains("hello"));

    // Cleanup
    let _ = std::fs::remove_file(test_file);
}
