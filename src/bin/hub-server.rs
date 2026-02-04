use tokio::sync::mpsc::unbounded_channel;
use tokio::{sync::broadcast, try_join};

use posenet_vr_hub::controller::Controller;
use posenet_vr_hub::grpc::proto::CameraInfo;
use posenet_vr_hub::grpc::proto::Snapshot;
use posenet_vr_hub::grpc::server::{GrpcConfig, GrpcServer};
use posenet_vr_hub::triangulator::{LabeledPoses3D, TriangulatorConfig};
use posenet_vr_hub::vrpn::server::{VrpnConfig, VrpnServer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    log::info!("Hub main start");

    let (cameras_tx, cameras_rx) = unbounded_channel::<CameraInfo>();
    let (snapshots_tx, snapshots_rx) = unbounded_channel::<Snapshot>();
    let (poses3d_bcast_tx, poses3d_bcast_rx) = broadcast::channel::<LabeledPoses3D>(100);

    let triangulator_config = TriangulatorConfig::default();
    let controller = Controller::new(
        triangulator_config,
        cameras_rx,
        snapshots_rx,
        poses3d_bcast_tx,
    );

    let grpc_config = GrpcConfig::default();
    let grpc_server = GrpcServer::new(grpc_config, cameras_tx, snapshots_tx);

    let vrpn_config = VrpnConfig::default();
    let mut vrpn_server = VrpnServer::new(vrpn_config, poses3d_bcast_rx);

    let run_controller = async move {
        match controller.run().await {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow::Error::msg(e.to_string())),
        }
    };
    let run_grpc = grpc_server.run();
    let run_vrpn = async move {
        match vrpn_server.run().await {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow::Error::msg(e.to_string())),
        }
    };
    try_join!(run_controller, run_grpc, run_vrpn)?;
    log::info!("Hub main end");

    Ok(())
}
