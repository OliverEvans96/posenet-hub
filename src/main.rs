use async_std::channel;
use std::error::Error;
use std::time::Duration;
use tokio::try_join;

mod controller;
mod grpc;
mod utils;
mod vrpn;

use controller::ControllerAlgorithm;
use grpc::proto::Pose3D;
use grpc::server::{LabeledPose2D, NamedCameraInfo};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (cameras_tx, cameras_rx) = channel::unbounded::<NamedCameraInfo>();
    let (poses2d_tx, poses2d_rx) = channel::unbounded::<LabeledPose2D>();
    let (poses3d_tx, poses3d_rx) = channel::unbounded::<Pose3D>();
    let controller = ControllerAlgorithm::LatestPoseTimeLimit(Duration::from_secs(5));
    try_join!(
        controller.run(cameras_rx, poses2d_rx, poses3d_tx),
        grpc::server::main(cameras_tx, poses2d_tx),
        vrpn::server::main(poses3d_rx),
    )?;

    Ok(())
}
