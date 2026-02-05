use std::path::PathBuf;
use structopt::StructOpt;

use posenet_vr_hub::grpc::client as grpc_client;
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;
use posenet_vr_hub::grpc::proto::Image;
use tonic::transport::Channel;

#[derive(Debug, StructOpt)]
struct CommonOpts {
    /// gRPC port
    #[structopt(short, long, default_value = "50051")]
    port: u16,

    /// Server address
    #[structopt(short, long, default_value = "localhost")]
    server: String,
}

// ============== Admin subcommands ==============

#[derive(Debug, StructOpt)]
enum AdminCommand {
    /// List all camera groups
    ListGroups {
        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// List cameras in a group
    ListCameras {
        /// Group name
        #[structopt(short, long)]
        group: String,

        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// Get camera info (calibration, etc.) for a camera
    GetCameraInfo {
        /// Group name
        #[structopt(short, long)]
        group: String,

        /// Camera name
        #[structopt(short, long)]
        camera: String,

        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// Start streaming for a group
    StreamControlStart {
        /// Group name
        #[structopt(short, long)]
        group: String,

        /// Include pose in stream
        #[structopt(long)]
        pose: bool,

        /// Include image in stream
        #[structopt(long)]
        image: bool,

        /// FPS (0 = default)
        #[structopt(long)]
        fps: Option<f32>,

        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// Stop streaming for a group
    StreamControlStop {
        /// Group name
        #[structopt(short, long)]
        group: String,

        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// Request snapshots from a group (or a single camera)
    TakeSnapshots {
        /// Group name
        #[structopt(short, long)]
        group: String,

        /// Optional camera name (default: whole group)
        #[structopt(short, long)]
        camera: Option<String>,

        /// Include pose in snapshot
        #[structopt(long)]
        pose: bool,

        /// Include image in snapshot
        #[structopt(long)]
        image: bool,

        /// Request 3D pose
        #[structopt(long)]
        pose3d: bool,

        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// Get current cached snapshot for a group or camera
    GetCurrent {
        /// Group name
        #[structopt(short, long)]
        group: String,

        /// Optional camera name (default: whole group)
        #[structopt(short, long)]
        camera: Option<String>,

        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// Run calibration on a group or camera
    Calibrate {
        /// Group name
        #[structopt(short, long)]
        group: String,

        /// Optional camera name (default: whole group)
        #[structopt(short, long)]
        camera: Option<String>,

        /// Run extrinsic calibration
        #[structopt(long)]
        extrinsic: bool,

        /// Run intrinsic calibration
        #[structopt(long)]
        intrinsic: bool,

        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// Ping cameras (optional group/camera; none = ping none)
    Ping {
        /// Group name (if omitted, pings no cameras)
        #[structopt(short, long)]
        group: Option<String>,

        /// Camera name (requires group)
        #[structopt(short, long)]
        camera: Option<String>,

        /// Timeout in seconds
        #[structopt(long)]
        timeout: Option<u64>,

        #[structopt(flatten)]
        common: CommonOpts,
    },

    /// Tell cameras to update themselves (pull latest code)
    UpdateCameras {
        /// Group name
        #[structopt(short, long)]
        group: String,

        /// Optional camera name (default: whole group)
        #[structopt(short, long)]
        camera: Option<String>,

        #[structopt(flatten)]
        common: CommonOpts,
    },
}

// ============== Camera subcommands ==============

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
        /// Optional path to an image file to send when requested
        #[structopt(short, long)]
        #[structopt(parse(from_os_str))]
        image: Option<PathBuf>,

        #[structopt(flatten)]
        camera: CameraOpts,

        #[structopt(flatten)]
        common: CommonOpts,
    },
}

// ============== Top-level ==============

#[derive(Debug, StructOpt)]
#[structopt(
    name = "grpc-client",
    version = "0.1.0",
    about = "gRPC client to connect to posenet-hub server"
)]
enum GrpcClientCommand {
    /// Admin operations: list groups/cameras, stream control, snapshots, calibrate, ping
    Admin {
        #[structopt(subcommand)]
        cmd: AdminCommand,
    },

    /// Act as a camera client (stream poses, offer snapshots)
    Camera {
        #[structopt(subcommand)]
        cmd: CameraCommand,
    },
}

impl AdminCommand {
    fn common_opts(&self) -> &CommonOpts {
        match self {
            AdminCommand::ListGroups { common, .. } => common,
            AdminCommand::ListCameras { common, .. } => common,
            AdminCommand::GetCameraInfo { common, .. } => common,
            AdminCommand::StreamControlStart { common, .. } => common,
            AdminCommand::StreamControlStop { common, .. } => common,
            AdminCommand::TakeSnapshots { common, .. } => common,
            AdminCommand::GetCurrent { common, .. } => common,
            AdminCommand::Calibrate { common, .. } => common,
            AdminCommand::Ping { common, .. } => common,
            AdminCommand::UpdateCameras { common, .. } => common,
        }
    }
}

impl CameraCommand {
    fn common_opts(&self) -> &CommonOpts {
        match self {
            CameraCommand::StreamPoses { common, .. } => common,
            CameraCommand::OfferSnapshots { common, .. } => common,
        }
    }
}

async fn connect(server: &str, port: u16) -> anyhow::Result<HubServiceClient<Channel>> {
    let addr = format!("http://{}:{}", server, port);
    log::info!("Connecting to server at '{}'", addr);
    let client = HubServiceClient::connect(addr).await?;
    log::info!("Connected successfully");
    Ok(client)
}

// ---------- Admin handlers ----------

fn grpc_err(e: Box<dyn std::error::Error + Send + Sync>) -> anyhow::Error {
    anyhow::Error::msg(e.to_string())
}

async fn run_admin_list_groups(client: &mut HubServiceClient<Channel>) -> anyhow::Result<()> {
    let groups = grpc_client::list_groups(client).await.map_err(grpc_err)?;
    for g in &groups {
        println!("{}", g);
    }
    if groups.is_empty() {
        println!("(no groups)");
    }
    Ok(())
}

async fn run_admin_list_cameras(
    client: &mut HubServiceClient<Channel>,
    group: String,
) -> anyhow::Result<()> {
    let cameras = grpc_client::list_cameras(client, group.clone()).await.map_err(grpc_err)?;
    for c in &cameras {
        println!("{}", c);
    }
    if cameras.is_empty() {
        println!("(no cameras in group '{}')", group);
    }
    Ok(())
}

async fn run_admin_get_camera_info(
    client: &mut HubServiceClient<Channel>,
    group: String,
    camera: String,
) -> anyhow::Result<()> {
    let which = grpc_client::build_camera_identifier(group, camera);
    let info = grpc_client::get_camera_info(client, which).await.map_err(grpc_err)?;
    println!("{:#?}", info);
    Ok(())
}

async fn run_admin_stream_control_start(
    client: &mut HubServiceClient<Channel>,
    group: String,
    pose: bool,
    image: bool,
    fps: Option<f32>,
) -> anyhow::Result<()> {
    let status = grpc_client::stream_control_start(client, group, pose, image, fps)
        .await
        .map_err(grpc_err)?;
    println!("is_streaming: {}", status.is_streaming);
    Ok(())
}

async fn run_admin_stream_control_stop(
    client: &mut HubServiceClient<Channel>,
    group: String,
) -> anyhow::Result<()> {
    let status = grpc_client::stream_control_stop(client, group).await.map_err(grpc_err)?;
    println!("is_streaming: {}", status.is_streaming);
    Ok(())
}

async fn run_admin_take_snapshots(
    client: &mut HubServiceClient<Channel>,
    group: String,
    camera: Option<String>,
    pose: bool,
    image: bool,
    pose3d: bool,
) -> anyhow::Result<()> {
    let req = grpc_client::build_server_snapshot_request(
        group,
        camera,
        pose,
        image,
        pose3d,
    );
    let resp = grpc_client::take_snapshots(client, req).await.map_err(grpc_err)?;
    println!("snapshot_id: {}", resp.snapshot_id);
    println!("snapshots: {}", resp.snapshots.len());
    Ok(())
}

async fn run_admin_get_current(
    client: &mut HubServiceClient<Channel>,
    group: String,
    camera: Option<String>,
) -> anyhow::Result<()> {
    let which = grpc_client::build_camera_identifier(group, camera.unwrap_or_default());
    let resp = grpc_client::get_current(client, which).await.map_err(grpc_err)?;
    println!("snapshot_id: {}", resp.snapshot_id);
    println!("snapshots: {}", resp.snapshots.len());
    Ok(())
}

async fn run_admin_calibrate(
    client: &mut HubServiceClient<Channel>,
    group: String,
    camera: Option<String>,
    extrinsic: bool,
    intrinsic: bool,
) -> anyhow::Result<()> {
    let req = grpc_client::build_calibration_request(
        group,
        camera,
        extrinsic,
        intrinsic,
    );
    let resp = grpc_client::calibrate(client, req).await.map_err(grpc_err)?;
    println!("states: {}", resp.states.len());
    for s in &resp.states {
        if let Some(id) = &s.which_camera {
            println!("  {} / {}", id.group_name, id.camera_name);
        }
    }
    Ok(())
}

async fn run_admin_ping(
    client: &mut HubServiceClient<Channel>,
    group: Option<String>,
    camera: Option<String>,
    timeout: Option<u64>,
) -> anyhow::Result<()> {
    let which_camera = match (group, camera) {
        (Some(g), c) => Some(grpc_client::build_camera_identifier(g, c.unwrap_or_default())),
        (None, _) => None,
    };
    let req = grpc_client::build_ping_request(which_camera, timeout);
    let resp = grpc_client::ping(client, req).await.map_err(grpc_err)?;
    println!("results: {}", resp.results.len());
    for r in &resp.results {
        println!("  {:#?}", r);
    }
    Ok(())
}

async fn run_admin_update_cameras(
    client: &mut HubServiceClient<Channel>,
    group: String,
    camera: Option<String>,
) -> anyhow::Result<()> {
    let which = grpc_client::build_camera_identifier(group, camera.unwrap_or_default());
    let req = grpc_client::build_update_cameras_request(which);
    let resp = grpc_client::update_cameras(client, req).await.map_err(grpc_err)?;
    println!("cameras_updated: {}", resp.cameras_updated);
    Ok(())
}

// ---------- Camera handlers ----------

async fn stream_poses(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> anyhow::Result<()> {
    log::info!("Sending hello");
    let name = grpc_client::hello(client, group_name.clone())
        .await
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    log::info!("Streaming poses");
    grpc_client::stream_poses(client, group_name, name)
        .await
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok(())
}

async fn offer_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
    image_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    log::info!("Waiting for snapshot request in group '{}'", &group_name);
    let image_data = if let Some(p) = image_path {
        Some(Image::from_path(&p)?)
    } else {
        None
    };
    grpc_client::offer_snapshots(client, group_name, image_data)
        .await
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok(())
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let opts = GrpcClientCommand::from_args();

    let (server, port) = match &opts {
        GrpcClientCommand::Admin { cmd } => {
            let c = cmd.common_opts();
            (c.server.as_str(), c.port)
        }
        GrpcClientCommand::Camera { cmd } => {
            let c = cmd.common_opts();
            (c.server.as_str(), c.port)
        }
    };

    let mut client = connect(server, port).await?;

    match opts {
        GrpcClientCommand::Admin { cmd } => match cmd {
            AdminCommand::ListGroups { .. } => run_admin_list_groups(&mut client).await?,
            AdminCommand::ListCameras { group, .. } => {
                run_admin_list_cameras(&mut client, group).await?
            }
            AdminCommand::GetCameraInfo { group, camera, .. } => {
                run_admin_get_camera_info(&mut client, group, camera).await?
            }
            AdminCommand::StreamControlStart {
                group, pose, image, fps, ..
            } => {
                run_admin_stream_control_start(&mut client, group, pose, image, fps).await?
            }
            AdminCommand::StreamControlStop { group, .. } => {
                run_admin_stream_control_stop(&mut client, group).await?
            }
            AdminCommand::TakeSnapshots {
                group,
                camera,
                pose,
                image,
                pose3d,
                ..
            } => {
                run_admin_take_snapshots(&mut client, group, camera, pose, image, pose3d).await?
            }
            AdminCommand::GetCurrent { group, camera, .. } => {
                run_admin_get_current(&mut client, group, camera).await?
            }
            AdminCommand::Calibrate {
                group,
                camera,
                extrinsic,
                intrinsic,
                ..
            } => {
                run_admin_calibrate(&mut client, group, camera, extrinsic, intrinsic).await?
            }
            AdminCommand::Ping {
                group, camera, timeout, ..
            } => run_admin_ping(&mut client, group, camera, timeout).await?,
            AdminCommand::UpdateCameras { group, camera, .. } => {
                run_admin_update_cameras(&mut client, group, camera).await?
            }
        },
        GrpcClientCommand::Camera { cmd } => match cmd {
            CameraCommand::StreamPoses { camera, .. } => {
                stream_poses(&mut client, camera.group).await?
            }
            CameraCommand::OfferSnapshots { camera, image, .. } => {
                offer_snapshots(&mut client, camera.group, image).await?
            }
        },
    }

    log::info!("Done");
    Ok(())
}
