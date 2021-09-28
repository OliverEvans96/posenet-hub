use std::error::Error;
use std::path;
use structopt::StructOpt;

use posenet_vr_hub::grpc::client as grpc_client;
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;
use tonic::transport::Channel;

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
    GetSnapshots {
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
            GrpcClientCommand::GetSnapshots { common, .. } => common,
        }
    }
}

async fn connect(server: &str, port: u16) -> Result<HubServiceClient<Channel>, Box<dyn Error>> {
    let addr = format!("http://{}:{}", server, port);
    log::info!("Connecting to server at '{}'", addr);
    let client_result = HubServiceClient::connect(addr).await?;
    log::info!("Connected successfully");

    Ok(client_result)
}

async fn stream_poses(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<(), Box<dyn Error>> {
    log::info!("Sending hello");
    let name = grpc_client::hello(client, group_name.clone()).await?;
    log::info!("Streaming poses");
    grpc_client::stream_poses(client, group_name, name).await?;

    Ok(())
}

async fn offer_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<(), Box<dyn Error>> {
    log::info!("Waiting for snapshot request in group '{}'", &group_name);
    grpc_client::offer_snapshots(client, group_name).await?;

    Ok(())
}

async fn get_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<(), Box<dyn Error>> {
    log::info!("Getting snapshots");
    let snapshots = grpc_client::get_snapshots(client, group_name).await?;
    log::info!("Got snapshots: {:#?}", snapshots);

    Ok(())
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    env_logger::init();

    let opts = GrpcClientCommand::from_args();
    let common_opts = opts.get_common_opts();
    let mut client = connect(&common_opts.server, common_opts.port).await?;

    match opts {
        GrpcClientCommand::Camera { cmd } => match cmd {
            CameraCommand::StreamPoses { camera, .. } => {
                stream_poses(&mut client, camera.group).await?
            }
            CameraCommand::OfferSnapshots { camera, .. } => {
                offer_snapshots(&mut client, camera.group).await?
            }
        },
        GrpcClientCommand::GetSnapshots { group, .. } => get_snapshots(&mut client, group).await?,
    };

    log::info!("Done");

    Ok(())
}
