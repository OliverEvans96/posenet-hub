use std::error::Error;
use std::fs;
use std::path::PathBuf;
use structopt::StructOpt;

use posenet_vr_hub::grpc::client as grpc_client;
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;
use posenet_vr_hub::grpc::proto::ImageData;
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

    /// Stand by and offer to take phony snapshots at the server's request
    OfferSnapshots {
        /// Optional path to an image file to send when requested.
        /// Otherwise, random RGB pixels will be generated.
        #[structopt(short, long)]
        #[structopt(parse(from_os_str))]
        image: Option<PathBuf>,

        #[structopt(flatten)]
        camera: CameraOpts,

        #[structopt(flatten)]
        common: CommonOpts,
    },
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

        /// Base directory where new output directory should be created
        /// with returned images and data. Will be created if not present.
        /// Parent direcories will not be created (as in mkdir -p).
        #[structopt(short, long, default_value = "snapshots")]
        #[structopt(parse(from_os_str))]
        output: PathBuf,

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
    image_path: Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    log::info!("Waiting for snapshot request in group '{}'", &group_name);

    let image_data = if let Some(image_path) = image_path {
        Some(ImageData::from_path(&image_path)?)
    } else {
        None
    };

    grpc_client::offer_snapshots(client, group_name, image_data).await?;
    Ok(())
}

async fn get_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
    output_path: PathBuf,
) -> Result<(), Box<dyn Error>> {
    log::info!("Getting snapshots");
    // TODO: Stream snapshots instead?
    let snapshots_response = grpc_client::get_snapshots(client, group_name).await?;
    log::info!("Got {} snapshots.", snapshots_response.messages.len());

    // Print snapshot id to stdout
    println!("{}", snapshots_response.snapshot_id);

    // TODO: Reconstruct 3D poses also?

    // Create base directory if it doesn't exist
    if !output_path.exists() {
        fs::create_dir(&output_path)?;
    }

    // Create directory for this snapshot
    // throws an error if it already exists,
    // which shouldn't happen because snapshot_ids
    // are randomly generated UUIDs
    let snapshot_dir_path = output_path.join(snapshots_response.snapshot_id);
    fs::create_dir(&snapshot_dir_path)?;

    // TODO: split some of this into a separate function
    // Loop over snapshots
    for message in snapshots_response.messages {
        // Get image data from snapshot message
        if let Some(img) = message.image {
            if let Some(image_buf) =
                image::ImageBuffer::<image::Rgb<_>, _>::from_raw(img.width, img.height, img.data)
            {
                let image_filename = format!("{}.jpg", message.camera_name);
                let image_path = snapshot_dir_path.join(image_filename);

                // Write image data to file
                image_buf.save(image_path)?;
            } else {
                log::error!("Couldn't construct image buffer - too much data");
            }
        } else {
            log::warn!("Snapshot contained no image!");
        }

        // Write pose data to file
        let pose_filename = format!("{}.yaml", message.camera_name);
        let pose_path = snapshot_dir_path.join(pose_filename);
        // TODO: Maybe don't need to exit completely if one of these fails?
        // could handle errors more gracefully
        let pose_file = fs::File::create(pose_path)?;
        serde_yaml::to_writer(pose_file, &message.poses)?;
    }

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
            CameraCommand::OfferSnapshots { camera, image, .. } => {
                offer_snapshots(&mut client, camera.group, image).await?
            }
        },
        GrpcClientCommand::GetSnapshots { group, output, .. } => {
            get_snapshots(&mut client, group, output).await?
        }
    };

    log::info!("Done");

    Ok(())
}
