use structopt::StructOpt;

use posenet_vr_hub::grpc::client as grpc_client;
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;

#[derive(StructOpt)]
struct CommonOpts {
    /// gRPC Port
    #[structopt(short, long, default_value = "50051")]
    port: u16,
    /// Server address
    #[structopt(short, long, default_value = "localhost")]
    server: String,
}

#[derive(StructOpt)]
#[structopt(
    name = "grpc-client",
    version = "0.1.0",
    about = "gRPC client to connect to posenet-hub server"
)]
enum GrpcClientOpts {
    /// Act as a camera, streaming randomly generated poses to the server
    Stream {
        /// Group name for client
        #[structopt(short, long, default_value = "grpc_client")]
        group: String,
        #[structopt(flatten)]
        common: CommonOpts,
    },
    /// Act as a non-camera client, and request a snapshot
    Snapshot {
        /// Group name for client
        #[structopt(short, long)]
        group: String,
        #[structopt(flatten)]
        common: CommonOpts,
    },
    // TODO: non-camera snapshot client
}

impl GrpcClientOpts {
    /// Any variant should have the common options, so provide a unified interface
    fn get_common(&self) -> &CommonOpts {
        match self {
            GrpcClientOpts::Stream { common, .. } => common,
            GrpcClientOpts::Snapshot { common, .. } => common,
        }
    }
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    env_logger::init();

    let opts = GrpcClientOpts::from_args();
    let common_opts = opts.get_common();

    let addr = format!("http://{}:{}", common_opts.server, common_opts.port);
    log::info!("Connecting to server at '{}'", addr);
    let mut client = HubServiceClient::connect(addr).await?;

    match opts {
        GrpcClientOpts::Stream { group, .. } => {
            log::info!("Sending hello");
            let name = grpc_client::hello(&mut client, &group).await?;
            log::info!("Streaming poses");
            grpc_client::stream_poses(&mut client, &group, &name).await?;
        }
        GrpcClientOpts::Snapshot { group, .. } =>{
          log::info!("Waiting for snapshot request");
          // TODO: This subcommand is actually the non-camera version
          grpc_client::wait_for_snapshot_request(&mut client, group).await?;
        },
    };

    log::info!("Done");

    Ok(())
}
