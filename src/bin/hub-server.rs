use async_std::channel;
use channel::unbounded;
use std::error::Error;
use tokio::try_join;

use posenet_vr_hub::controller::{Controller, ControllerConfig};
use posenet_vr_hub::grpc::proto::Pose3D;
use posenet_vr_hub::grpc::server::{LabeledPose2D, NamedCameraInfo};
use posenet_vr_hub::{grpc, vrpn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (cameras_tx, cameras_rx) = channel::unbounded::<NamedCameraInfo>();
    let (poses2d_tx, poses2d_rx) = channel::unbounded::<LabeledPose2D>();
    let (poses3d_tx, poses3d_rx) = channel::unbounded::<Pose3D>();
    let config = ControllerConfig::default();
    let controller = Controller::new(cameras_rx, poses2d_rx, poses3d_tx, config);
    println!("Hub main start");

    try_join!(
        controller.run(),
        grpc::server::main(cameras_tx, poses2d_tx),
        vrpn::server::main(poses3d_rx),
    )?;
    println!("Hub main end");

    Ok(())
}
