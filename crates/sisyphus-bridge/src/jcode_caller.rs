use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{timeout, Duration};
use sisyphus_tools_agent::jcode_executor::JcodeToolCaller;

pub struct DebugSocketToolCaller {
    socket_path: String,
}

impl DebugSocketToolCaller {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }
}

#[async_trait]
impl JcodeToolCaller for DebugSocketToolCaller {
    async fn call_tool(&self, tool_id: &str, args: Vec<u8>) -> Result<Vec<u8>, String> {
        let mut last_error = String::new();
        let max_retries = 3;
        let connect_timeout = Duration::from_secs(2);
        let io_timeout = Duration::from_secs(5);

        for attempt in 1..=max_retries {
            let connect_res = timeout(connect_timeout, UnixStream::connect(&self.socket_path)).await;
            
            match connect_res {
                Ok(Ok(mut stream)) => {
                    let args_json = String::from_utf8_lossy(&args).to_string();
                    let command = format!("tool:{} {}", tool_id, args_json);
                    
                    let write_res = timeout(io_timeout, async {
                        stream.write_all(command.as_bytes()).await?;
                        stream.write_all(b"\n").await
                    }).await;

                    if let Ok(Ok(_)) = write_res {
                        let mut response = Vec::new();
                        if let Ok(Ok(_)) = timeout(io_timeout, stream.read_to_end(&mut response)).await {
                            return Ok(response);
                        }
                        last_error = "Failed to read response from jcode socket".into();
                    } else {
                        last_error = "Failed to write command to jcode socket".into();
                    }
                }
                Ok(Err(e)) => last_error = format!("Connection error: {}", e),
                Err(_) => last_error = "Connection timeout".into(),
            }
            
            if attempt < max_retries {
                tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
            }
        }

        Err(format!("Jcode tool call failed after {} attempts. Last error: {}", max_retries, last_error))
    }
}
