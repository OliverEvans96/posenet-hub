use clap::App;

use posenet_vr_hub::grpc::client::{hello, stream_poses};
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new("grpc-client")
        .version("0.1.0")
        .about("Test grpc-client sends random 2D poses to posenet hub-server.")
        .args_from_usage(
            "-s, --server=[server] 'Server address (default: localhost)'
                            -p, --port=[port]      'Grpc port (default: 50051)'
                            -g, --group=[group]    'Client group name (default: grpc_client)'",
        )
        .get_matches();

    // Get config flags or defaults
    let server = matches.value_of("server").unwrap_or("127.0.0.1");
    let port = matches.value_of("port").unwrap_or("50051");
    let group = matches.value_of("group").unwrap_or("grpc_client");

    // TODO: subcommands for streaming vs snapshot?

    let addr = format!("http://{}:{}", server, port);
    println!("Connecting");
    let mut client = HubServiceClient::connect(addr).await?;
    println!("Sending hello");
    let name = hello(&mut client, group).await?;
    println!("Streaming poses");
    stream_poses(&mut client, group, &name).await?;
    println!("Done");

    Ok(())
}
