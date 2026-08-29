use sisyphus_types::{TestExecutor as TestExecutorTrait, TestResult, TypesError};
use std::path::Path;

pub struct TestExecutor {
    // We can add configuration here, like default test commands for different languages
}

impl TestExecutor {
    /// Create an empty test registry (no test cases loaded).
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for TestExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl TestExecutorTrait for TestExecutor {
    async fn run_test(
        &self,
        command: &str,
        workdir: Option<&str>,
    ) -> Result<TestResult, TypesError> {
        let workdir_ref = workdir.map(Path::new);
        let timeout_ms = 60000;

        let output = sisyphus_tools_system::bash::execute(command, workdir_ref, timeout_ms)
            .await
            .map_err(|e| TypesError::NodeExecutionError(format!("Test execution failed: {e}")))?;

        let success = output.exit_code == 0;
        let error_summary = if !success {
            let full_log = format!("{}\n{}", output.stdout, output.stderr);
            if let Some(line) = full_log
                .lines()
                .find(|l| l.to_uppercase().contains("ERROR") || l.to_uppercase().contains("FAIL"))
            {
                Some(line.to_string())
            } else {
                Some("Test failed without explicit error message in logs".into())
            }
        } else {
            None
        };

        Ok(TestResult {
            success,
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.exit_code,
            duration_ms: output.duration.as_millis(),
            error_summary,
        })
    }
}
