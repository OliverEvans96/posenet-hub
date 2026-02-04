use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::unbounded_channel;
use tokio::{sync::broadcast, try_join};

use clap::Parser;
use posenet_vr_hub::controller::Controller;
use posenet_vr_hub::grpc::proto::CameraInfo;
use posenet_vr_hub::grpc::proto::Snapshot;
use posenet_vr_hub::grpc::server::{GrpcConfig, GrpcServer, HubServer};
use posenet_vr_hub::triangulator::{LabeledPoses3D, PoseStreamUpdate, TriangulatorConfig};
use posenet_vr_hub::vrpn::server::{VrpnConfig, VrpnServer};
use posenet_vr_hub::websocket::{WebSocketConfig, WebSocketServer};

#[derive(Parser, Debug)]
#[clap(name = "hub-server", about = "PoseNet Hub: gRPC, optional WebSocket pose stream and VRPN")]
struct Opts {
    /// Enable WebSocket server for pose stream (default: disabled)
    #[clap(long)]
    ws: bool,

    /// Enable VRPN server for pose tracking (default: disabled)
    #[clap(long)]
    vrpn: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let opts = Opts::parse();
    log::info!("Hub main start (ws={}, vrpn={})", opts.ws, opts.vrpn);

    let (cameras_tx, cameras_rx) = unbounded_channel::<CameraInfo>();
    let (snapshots_tx, snapshots_rx) = unbounded_channel::<Snapshot>();
    let (stream_bcast_tx, _) = broadcast::channel::<PoseStreamUpdate>(100);

    let triangulator_config = TriangulatorConfig::default();
    let controller = Controller::new(
        triangulator_config,
        cameras_rx,
        snapshots_rx,
        stream_bcast_tx.clone(),
    );

    let hub = Arc::new(HubServer::new(cameras_tx, snapshots_tx));
    let grpc_config = GrpcConfig::default();
    let grpc_server = GrpcServer::new(grpc_config, hub.clone());

    let run_controller = async move {
        match controller.run().await {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow::Error::msg(e.to_string())),
        }
    };
    let run_grpc = grpc_server.run();

    // VRPN server holds C++ state that is !Send, so run it in a dedicated thread with its own runtime.
    if opts.vrpn {
        // Forward only 3D poses to VRPN (do not include camera metadata).
        let (poses3d_tx, _) = broadcast::channel::<LabeledPoses3D>(100);
        let mut stream_rx = stream_bcast_tx.subscribe();
        let poses3d_tx_forw = poses3d_tx.clone();
        std::thread::spawn(move || {
            let rt = Runtime::new().expect("VRPN forward runtime");
            rt.block_on(async move {
                loop {
                    match stream_rx.recv().await {
                        Ok(update) => {
                            let _ = poses3d_tx_forw.send(update.labeled_poses);
                        }
                        Err(e) => {
                            log::error!("VRPN forward recv error: {}", e);
                            break;
                        }
                    }
                }
            });
        });

        let vrpn_rx = poses3d_tx.subscribe();
        std::thread::spawn(move || {
            let rt = Runtime::new().expect("VRPN runtime");
            let mut vrpn_server = VrpnServer::new(VrpnConfig::default(), vrpn_rx);
            if let Err(e) = rt.block_on(vrpn_server.run()) {
                log::error!("VRPN server error: {}", e);
            }
        });
    }

    let run_ws: Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> = if opts.ws {
        let ws_server = WebSocketServer::new(
            WebSocketConfig::default(),
            stream_bcast_tx.subscribe(),
            Some(hub.clone()),
        );
        Box::pin(async move {
            match ws_server.run().await {
                Ok(()) => Ok(()),
                Err(e) => Err(anyhow::Error::msg(e.to_string())),
            }
        })
    } else {
        Box::pin(std::future::pending())
    };

    try_join!(run_controller, run_grpc, run_ws)?;
    log::info!("Hub main end");

    Ok(())
}
