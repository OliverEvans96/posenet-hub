use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use posenet_vr_hub::triangulator::{LabeledPoses3D, PoseStreamUpdate};
use serde::Deserialize;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::join;
use tokio::sync::broadcast;

use posenet_vr_hub::grpc::proto::Pose3D;
use posenet_vr_hub::vrpn::server::{VrpnConfig, VrpnServer};

#[derive(Parser)]
struct Opts {
    #[clap(short)]
    input: PathBuf,
    #[clap(short, default_value = "3883")]
    port: u16,
    #[clap(long, default_value = "0.0.0.0")]
    ip: String,
    #[clap(short, default_value = "playback")]
    device_name: String,
    #[clap(short, default_value = "playback_group")]
    group_name: String,
}

#[derive(Deserialize)]
struct Frame {
    frame: usize,
    bones: Pose3D,
}

async fn pose_playback(
    input_path: &Path,
    poses_tx: broadcast::Sender<PoseStreamUpdate>,
    group_name: &str,
) -> anyhow::Result<()> {
    let input_file = File::open(input_path).await?;
    let mut line_reader = BufReader::new(input_file).lines();

    // Collect all frames
    let mut frames = Vec::new();
    while let Some(line) = line_reader.next_line().await? {
        let frame: Frame = serde_yaml::from_str(&line)?;
        frames.push(frame);
    }

    // Repeat frames forever
    loop {
        for frame in &frames {
            let labeled_poses = LabeledPoses3D {
                group_name: group_name.to_string(),
                poses: vec![frame.bones.clone()],
                time: Instant::now(),
            };
            let update = PoseStreamUpdate {
                labeled_poses,
                camera_views: vec![],
            };
            poses_tx.send(update)?;
            println!("sent frame {}", frame.frame);
            // TODO: Make framerate adjustable
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    dotenv::dotenv().ok();
    env_logger::init();

    println!("VRPN playback start");

    // Create communication channels
    let (poses3d_bcast_tx, poses3d_bcast_rx) = broadcast::channel::<PoseStreamUpdate>(100);

    // Create VRPN server
    let vrpn_config = VrpnConfig::new(&opts.device_name, &opts.ip, opts.port)?;
    let mut vrpn_server = VrpnServer::new(vrpn_config, poses3d_bcast_rx);

    let pose_fut = pose_playback(&opts.input, poses3d_bcast_tx, &opts.group_name);

    let (vrpn_res, playback_res) = join!(vrpn_server.run(), pose_fut);

    if let Err(err) = vrpn_res {
        eprintln!("VRPN Error: {}", err);
    }
    if let Err(err) = playback_res {
        eprintln!("Playback Error: {}", err);
    }

    println!("VRPN playback end");

    Ok(())
}
