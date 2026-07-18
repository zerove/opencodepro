pub mod executor;
pub mod executor_mcp;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_executor_type() {
        fn assert_type<T: 'static>() {}
        assert_type::<executor::BuiltinToolExecutor>();
    }

    #[test]
    fn test_mcp_executor_type() {
        fn assert_type<T: 'static>() {}
        assert_type::<executor_mcp::MCPToolExecutor>();
    }
}
