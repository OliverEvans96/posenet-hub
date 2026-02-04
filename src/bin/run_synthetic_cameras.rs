//! Synthetic cameras: stream fake pose observations from a CSV pose,
//! rotated and projected through virtual cameras, to the hub server.

use futures::StreamExt;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use structopt::StructOpt;
use tokio::sync::mpsc;
use tokio::time::interval;
use tonic::transport::Channel;
use tonic::Request;

use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;
use posenet_vr_hub::grpc::proto::{
    camera_control_command, camera_message, command_response, CalibrationParameters,
    CameraIdentifier, CameraInfo, CameraMessage, CommandResponse, CommandToken, SessionToken,
    Snapshot,
};
use posenet_vr_hub::synthetic_cameras::{
    load_pose_csv, points_to_pose3d, project_pose3d_to_pose2d, rotate_pose_around,
    rotation_around_z_rad, SyntheticCamerasConfig,
};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "run_synthetic_cameras",
    about = "Stream synthetic pose observations to the hub"
)]
struct Args {
    /// Path to YAML config (cameras, hub_url, pose CSV, etc.)
    #[structopt(short, long, parse(from_os_str))]
    config: PathBuf,

    /// Override: run for this many seconds then exit (overrides config)
    #[structopt(long)]
    duration_secs: Option<u64>,
}

struct CameraState {
    name: String,
    calibration: CalibrationParameters,
    which_camera: CameraIdentifier,
    snapshot_tx: mpsc::UnboundedSender<Snapshot>,
}

async fn run_camera_control_listener(
    mut client: HubServiceClient<Channel>,
    session_token: SessionToken,
    camera_name: String,
    group_name: String,
    mut snapshot_rx: mpsc::UnboundedReceiver<Snapshot>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let which_camera = CameraIdentifier {
        group_name: group_name.clone(),
        camera_name: camera_name.clone(),
    };
    let request = Request::new(session_token);
    let mut stream = client.camera_control(request).await?.into_inner();
    while let Some(msg) = stream.next().await {
        let cmd = msg?;
        match (&cmd.token, &cmd.command) {
            (Some(token), Some(camera_control_command::Command::StartStreaming(_))) => {
                let command_token = token.clone();
                let mut data_sink_client = client.clone();
                let which_cam = which_camera.clone();
                let rx = std::mem::replace(&mut snapshot_rx, mpsc::unbounded_channel().1);
                tokio::spawn(async move {
                    if let Err(e) =
                        run_camera_data_sink(&mut data_sink_client, command_token, which_cam, rx)
                            .await
                    {
                        log::error!("CameraDataSink error: {}", e);
                    }
                });
            }
            (_, Some(camera_control_command::Command::Update(_))) => {
                log::info!(
                    "Update command received for {} (synthetic camera does not pull code)",
                    camera_name
                );
            }
            _ => {}
        }
    }
    Ok(())
}

async fn run_camera_data_sink(
    client: &mut HubServiceClient<Channel>,
    command_token: CommandToken,
    _which_camera: CameraIdentifier,
    mut snapshot_rx: mpsc::UnboundedReceiver<Snapshot>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token_msg = CameraMessage {
        msg: Some(camera_message::Msg::Token(command_token)),
    };
    let stream = async_stream::stream! {
        yield token_msg;
        while let Some(snap) = snapshot_rx.recv().await {
            yield CameraMessage {
                msg: Some(camera_message::Msg::Response(CommandResponse {
                    response: Some(command_response::Response::Snapshot(snap)),
                })),
            };
        }
    };
    let request = Request::new(stream);
    let _response = client.camera_data_sink(request).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let args = Args::from_args();
    let config = SyntheticCamerasConfig::load_from_path(&args.config)
        .map_err(|e| anyhow::anyhow!("load config: {}", e))?;
    let duration_secs = args.duration_secs.or(config.duration_secs);

    let pose_path = if config.pose_csv_path.is_absolute() {
        config.pose_csv_path.clone()
    } else {
        std::env::current_dir()?.join(&config.pose_csv_path)
    };
    let points_original = load_pose_csv(&pose_path)?;
    let pose_center = points_original
        .iter()
        .fold(nalgebra::Point3::new(0.0, 0.0, 0.0), |acc, p| {
            nalgebra::Point3::new(acc.x + p.x, acc.y + p.y, acc.z + p.z)
        });
    let n = points_original.len() as f64;
    let pose_center =
        nalgebra::Point3::new(pose_center.x / n, pose_center.y / n, pose_center.z / n);

    let hub_url = config.hub_url.clone();
    let group_name = config.group_name.clone();
    let interval_duration = Duration::from_secs_f32(1.0 / config.fps);
    let rotation_speed = config.rotation_speed_rad_per_sec;

    let mut cameras = Vec::new();
    for (i, cam_cfg) in config.cameras.iter().enumerate() {
        let channel = Channel::from_shared(hub_url.clone())?.connect().await?;
        let mut client = HubServiceClient::new(channel);
        let camera_name = cam_cfg
            .name
            .clone()
            .unwrap_or_else(|| format!("synthetic_{}", i));
        let calibration = cam_cfg.calibration();
        let which_camera = CameraIdentifier {
            group_name: group_name.clone(),
            camera_name: camera_name.clone(),
        };
        let info = CameraInfo {
            which_camera: Some(which_camera.clone()),
            calibration: Some(calibration.clone()),
        };
        let request = Request::new(info);
        let session_token = client.hello(request).await?.into_inner();
        log::info!("Camera {} registered, session token received", camera_name);

        let (snapshot_tx, snapshot_rx) = mpsc::unbounded_channel();
        let session_token_listener = session_token.clone();
        let hub_url_listener = hub_url.clone();
        let group_listener = group_name.clone();
        let cam_name_listener = camera_name.clone();
        tokio::spawn(async move {
            let channel = Channel::from_shared(hub_url_listener)
                .unwrap()
                .connect()
                .await
                .unwrap();
            let client = HubServiceClient::new(channel);
            if let Err(e) = run_camera_control_listener(
                client,
                session_token_listener,
                cam_name_listener,
                group_listener,
                snapshot_rx,
            )
            .await
            {
                log::error!("CameraControl listener error: {}", e);
            }
        });

        cameras.push(CameraState {
            name: camera_name,
            calibration,
            which_camera,
            snapshot_tx,
        });
    }

    // Tell the hub to start streaming for this group so it sends StartStreaming to each camera.
    // Until this runs, cameras never open CameraDataSink and no snapshots reach the triangulator.
    let admin_channel = Channel::from_shared(hub_url.clone())?.connect().await?;
    let mut admin_client = HubServiceClient::new(admin_channel);
    posenet_vr_hub::grpc::client::stream_control_start(
        &mut admin_client,
        group_name.clone(),
        true,  // with_pose
        false, // with_image
        Some(config.fps),
    )
    .await
    .map_err(|e| anyhow::anyhow!("stream_control_start: {}", e))?;
    log::info!("StreamControl(StartStreaming) sent for group '{}'", group_name);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let start = Instant::now();
    let mut frame_interval = interval(interval_duration);

    loop {
        if let Some(secs) = duration_secs {
            if start.elapsed() >= Duration::from_secs(secs) {
                log::info!("Duration {}s reached, exiting", secs);
                break;
            }
        }

        frame_interval.tick().await;
        // Constant rate: angle from elapsed wall-clock time (radians per second around Z).
        // We must rotate from points_original each frame. Do NOT re-apply rotation to
        // already-rotated points (that would compound: effective angle would grow as
        // 1+2+...+n → quadratic speedup and apparent reversals).
        let angle_rad = rotation_speed * start.elapsed().as_secs_f64();
        let rot = rotation_around_z_rad(angle_rad);
        let mut points = points_original.clone();
        rotate_pose_around(&mut points, pose_center, &rot);
        let pose3d = points_to_pose3d(&points, 1.0);

        for cam in &cameras {
            match project_pose3d_to_pose2d(&pose3d, &cam.calibration) {
                Ok(pose2d) => {
                    let snapshot = Snapshot {
                        timestamp: Some(SystemTime::now().into()),
                        which_camera: Some(cam.which_camera.clone()),
                        poses: vec![pose2d],
                        image: None,
                    };
                    let _ = cam.snapshot_tx.send(snapshot);
                }
                Err(e) => {
                    log::warn!("Projection failed for {}: {}", cam.name, e);
                }
            }
        }
    }

    Ok(())
}
