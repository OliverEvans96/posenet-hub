use posenet_vr_hub::grpc::client::{hello, stream_poses};
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "http://127.0.0.1:50051";
    println!("Connecting");
    let mut client = HubServiceClient::connect(addr).await?;
    println!("Sending hello");
    let name = hello(&mut client).await?;
    println!("Streaming poses");
    stream_poses(&mut client, name).await?;
    println!("Done");

    Ok(())
}
