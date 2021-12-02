use async_std::channel;
// use std::error::Error;
use tokio::{sync::broadcast, try_join};

use posenet_vr_hub::controller::{BoxError, Controller};
use posenet_vr_hub::grpc::CameraInfo;
use posenet_vr_hub::grpc::server::Snapshot;
use posenet_vr_hub::grpc::server::{GrpcConfig, GrpcServer};
use posenet_vr_hub::triangulator::{LabeledPoses3D, TriangulatorConfig};
use posenet_vr_hub::vrpn::server::{VrpnConfig, VrpnServer};

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    dotenv::dotenv().ok();
    env_logger::init();

    log::info!("Hub main start");

    // Create communication channels
    let (cameras_tx, cameras_rx) = channel::unbounded::<CameraInfo>();
    let (snapshots_tx, snapshots_rx) = channel::unbounded::<Snapshot>();
    // let (poses3d_tx, poses3d_rx) = channel::unbounded::<Option<Pose3D>>();
    let (poses3d_bcast_tx, poses3d_bcast_rx) = broadcast::channel::<LabeledPoses3D>(100);

    // Create controller
    let triangulator_config = TriangulatorConfig::default();
    let controller = Controller::new(triangulator_config, cameras_rx, snapshots_rx, poses3d_bcast_tx);

    // Create gRPC server
    let grpc_config = GrpcConfig::default();
    let grpc_server = GrpcServer::new(grpc_config, cameras_tx, snapshots_tx);

    // Create VRPN server
    let vrpn_config = VrpnConfig::default();
    let mut vrpn_server = VrpnServer::new(vrpn_config, poses3d_bcast_rx);

    // Run all three components concurrently
    try_join!(controller.run(), grpc_server.run(), vrpn_server.run())?;
    log::info!("Hub main end");

    Ok(())
}
