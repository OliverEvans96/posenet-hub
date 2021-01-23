use async_std::channel;
use std::error::Error;
use tokio::try_join;

use posenet_vr_hub::controller::{Controller, ControllerConfig};
use posenet_vr_hub::grpc::proto::Pose3D;
use posenet_vr_hub::grpc::server::{GrpcConfig, GrpcServer};
use posenet_vr_hub::grpc::server::{LabeledPose2D, NamedCameraInfo};
use posenet_vr_hub::vrpn::server::{VrpnConfig, VrpnServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Hub main start");

    // Create communication channels
    let (cameras_tx, cameras_rx) = channel::unbounded::<NamedCameraInfo>();
    let (poses2d_tx, poses2d_rx) = channel::unbounded::<LabeledPose2D>();
    let (poses3d_tx, poses3d_rx) = channel::unbounded::<Pose3D>();

    // Create controller
    let controller_config = ControllerConfig::default();
    let controller = Controller::new(controller_config, cameras_rx, poses2d_rx, poses3d_tx);

    // Create gRPC server
    let grpc_config = GrpcConfig::default();
    let grpc_server = GrpcServer::new(grpc_config, cameras_tx, poses2d_tx);

    // Create VRPN server
    let vrpn_config = VrpnConfig::default();
    let vrpn_server = VrpnServer::new(vrpn_config, poses3d_rx);

    // Run all three components concurrently
    try_join!(controller.run(), grpc_server.run(), vrpn_server.run())?;
    println!("Hub main end");

    Ok(())
}
