use sisyphus_bridge_api::sisyphus_bridge_client::SisyphusBridgeClient;
use sisyphus_bridge_api::ExecuteSopRequest;
use std::time::Instant;
use tokio::task::JoinSet;
use tonic::Request;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = tonic::transport::Channel::from_static("http://[::1]:50051")
        .connect()
        .await?;

    let client = SisyphusBridgeClient::new(channel);
    let mut set = JoinSet::new();

    let concurrency = 50; // Start with 50 concurrent requests
    let iterations = 100;

    println!(
        "🚀 Starting stress test: concurrency={}, total_req={}",
        concurrency,
        concurrency * iterations
    );
    let start = Instant::now();

    for _ in 0..concurrency {
        let mut client = client.clone();
        set.spawn(async move {
            for _ in 0..iterations {
                let request = Request::new(ExecuteSopRequest {
                    sop_id: "hello".into(),
                    args: std::collections::HashMap::new(),
                });
                let _ = client.execute_sop(request).await;
            }
        });
    }

    while let Some(_) = set.join_next().await {}

    let elapsed = start.elapsed();
    let total = concurrency * iterations;
    println!(
        "✅ Stress test completed: {} requests in {:?} ({:.0} req/s)",
        total,
        elapsed,
        total as f64 / elapsed.as_secs_f64()
    );

    Ok(())
}
