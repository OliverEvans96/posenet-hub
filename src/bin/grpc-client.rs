use structopt::StructOpt;

use posenet_vr_hub::grpc::client as grpc_client;
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;

#[derive(Debug, StructOpt)]
struct CommonOpts {
    /// gRPC Port
    #[structopt(short, long, default_value = "50051")]
    port: u16,
    /// Server address
    #[structopt(short, long, default_value = "localhost")]
    server: String,
}

#[derive(Debug, StructOpt)]
struct CameraOpts {
    /// Group name for client
    #[structopt(short, long, default_value = "grpc_client")]
    group: String,
}

#[derive(Debug, StructOpt)]
enum CameraCommand {
    /// Stream randomly generated poses to the server
    StreamPoses {
        #[structopt(flatten)]
        camera: CameraOpts,
        #[structopt(flatten)]
        common: CommonOpts,
    },
    /// Stand by and offer to take snapshots at the server's request
    OfferSnapshots {
        #[structopt(flatten)]
        camera: CameraOpts,
        #[structopt(flatten)]
        common: CommonOpts,
    },
}

impl CameraCommand {
    /// Any variant should have the common options, so provide a unified interface
    fn get_common_opts(&self) -> &CommonOpts {
        match self {
            CameraCommand::StreamPoses { common, .. } => common,
            CameraCommand::OfferSnapshots { common, .. } => common,
        }
    }
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "grpc-client",
    version = "0.1.0",
    about = "gRPC client to connect to posenet-hub server"
)]
enum GrpcClientCommand {
    /// Act as a camera client
    Camera {
        #[structopt(subcommand)]
        cmd: CameraCommand,
    },

    /// Act as a non-camera client, and request a
    /// snapshot from currently connected cameras
    GetSnapshot {
        /// Camera group to request snapshots from
        #[structopt(short, long)]
        group: String,
        #[structopt(flatten)]
        common: CommonOpts,
    },
}

impl GrpcClientCommand {
    /// Any variant should have the common options, so provide a unified interface
    fn get_common_opts(&self) -> &CommonOpts {
        match self {
            GrpcClientCommand::Camera { cmd, .. } => cmd.get_common_opts(),
            GrpcClientCommand::GetSnapshot { common, .. } => common,
        }
    }
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    env_logger::init();

    let opts = GrpcClientCommand::from_args();
    let common_opts = opts.get_common_opts();

    let addr = format!("http://{}:{}", common_opts.server, common_opts.port);
    log::info!("Connecting to server at '{}'", addr);
    let mut client = HubServiceClient::connect(addr).await?;

    match opts {
        GrpcClientCommand::Camera { cmd } => match cmd {
            CameraCommand::StreamPoses { camera, .. } => {
                log::info!("Sending hello");
                let name = grpc_client::hello(&mut client, camera.group.clone()).await?;
                log::info!("Streaming poses");
                grpc_client::stream_poses(&mut client, camera.group, name).await?;
            }
            CameraCommand::OfferSnapshots { camera, .. } => {
                log::info!("Waiting for snapshot request");
                grpc_client::wait_for_snapshot_request(&mut client, camera.group).await?;
            }
        },
        GrpcClientCommand::GetSnapshot { group, .. } => {
            // TODO
            unimplemented!()
        }
    };

    log::info!("Done");

    Ok(())
}
