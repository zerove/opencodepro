use sisyphus_bridge_api::sisyphus_bridge_client::SisyphusBridgeClient;
use sisyphus_bridge_api::ExecuteSopRequest;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = SisyphusBridgeClient::connect("http://[::1]:50051").await?;

    let request = tonic::Request::new(ExecuteSopRequest {
        sop_id: "hello".into(),
        args: std::collections::HashMap::new(),
    });

    println!("Sending ExecuteSOP request for 'hello'...");
    let response = client.execute_sop(request).await?;

    println!("RESPONSE: {:?}", response.into_inner());

    Ok(())
}
