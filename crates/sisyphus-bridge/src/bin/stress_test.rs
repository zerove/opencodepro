use tonic::Request;
use sisyphus_bridge::sisyphus_bridge_client::SisyphusBridgeClient;
use sisyphus_bridge::ExecuteSopRequest;
use tokio::task::JoinSet;
use std::time::Instant;

pub mod sisyphus_bridge {
    tonic::include_proto!("sisyphus.bridge");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = tonic::transport::Channel::from_static("http://[::1]:50051")
        .connect()
        .await?;
    
    let client = SisyphusBridgeClient::new(channel);
    let mut set = JoinSet::new();
    
    let concurrency = 50; // Start with 50 concurrent requests
    let iterations = 100;
    
    println!("🚀 Starting stress test: concurrency={}, total_req={}", concurrency, concurrency * iterations);
    let start = Instant::now();

    for _ in 0..concurrency {
        let mut client_clone = client.clone();
        set.spawn(async move {
            let mut success = 0;
            let mut failure = 0;
            for j in 0..iterations {
                let req = Request::new(ExecuteSopRequest {
                    sop_id: if j % 2 == 0 { "fast_tool".into() } else { "evil_tool".into() },
                    args: std::collections::HashMap::new(),
                });
                
                match client_clone.execute_sop(req).await {
                    Ok(_) => success += 1,
                    Err(_) => failure += 1,
                }
            }
            (success, failure)
        });
    }

    let mut total_success = 0;
    let mut total_failure = 0;
    while let Some(res) = set.join_next().await {
        let (s, f) = res?;
        total_success += s;
        total_failure += f;
    }

    let duration = start.elapsed();
    println!("✅ Stress test completed in {:?}", duration);
    println!("Total Success: {}, Total Failure: {}", total_success, total_failure);
    println!("Req/sec: {:.2}", (total_success + total_failure) as f64 / duration.as_secs_f64());

    Ok(())
}
