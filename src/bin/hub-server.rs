use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::runtime::Runtime;
use tokio::signal;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::broadcast;

use std::path::PathBuf;

use parking_lot::RwLock;
use clap::Parser;
use posenet_vr_hub::config::HubConfig;
use posenet_vr_hub::controller::Controller;
use posenet_vr_hub::grpc::proto::CameraInfo;
use posenet_vr_hub::grpc::proto::Snapshot;
use posenet_vr_hub::grpc::server::{GrpcServer, HubServer};
use posenet_vr_hub::http_server::HttpServer;
use posenet_vr_hub::recording::RecordingState;
use posenet_vr_hub::triangulator::{LabeledPoses3D, PoseStreamUpdate};
use posenet_vr_hub::vrpn::server::VrpnServer;
use posenet_vr_hub::websocket::WebSocketServer;

/// Time window for "second Ctrl-C" to trigger exit.
const CTRL_C_EXIT_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Parser, Debug)]
#[clap(name = "hub-server", about = "PoseNet Hub: gRPC, optional WebSocket pose stream and VRPN")]
struct Opts {
    /// Path to TOML config file. If omitted, uses POSENET_CONFIG env or default in-memory config.
    #[clap(long)]
    config: Option<PathBuf>,

    /// Enable WebSocket server for pose stream (also enabled via config [websocket] enabled = true)
    #[clap(long)]
    ws: bool,

    /// Enable VRPN server for pose tracking (also enabled via config [vrpn] enabled = true)
    #[clap(long)]
    vrpn: bool,

    /// Disable 2D pose smoothing (use raw poses for matching and camera_views). Overrides config.
    #[clap(long)]
    no_smoothing: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let opts = Opts::parse();

    let config_path = opts
        .config
        .or_else(|| std::env::var("POSENET_CONFIG").ok().map(PathBuf::from));
    let mut hub_config = match &config_path {
        Some(p) => HubConfig::load_path(p)?,
        None => HubConfig::default_resolved()?,
    };
    if opts.no_smoothing {
        hub_config.triangulator.smoothing = None;
        log::info!("Smoothing disabled by --no-smoothing");
    }

    let ws_enabled = opts.ws || hub_config.websocket_enabled;
    let vrpn_enabled = opts.vrpn || hub_config.vrpn_enabled;
    log::info!(
        "Hub main start (ws={}, vrpn={}). First Ctrl-C reloads config, second Ctrl-C exits.",
        ws_enabled,
        vrpn_enabled
    );

    let (cameras_tx, cameras_rx) = unbounded_channel::<CameraInfo>();
    let (snapshots_tx, snapshots_rx) = unbounded_channel::<Snapshot>();
    let (stream_bcast_tx, _) = broadcast::channel::<PoseStreamUpdate>(100);

    let config_arc = Arc::new(RwLock::new(hub_config));
    let (config_reload_tx, _) = broadcast::channel::<posenet_vr_hub::triangulator::TriangulatorConfig>(4);

    let controller = if config_path.is_some() {
        Controller::new_with_reload(
            config_arc.clone(),
            config_reload_tx.clone(),
            cameras_rx,
            snapshots_rx,
            stream_bcast_tx.clone(),
        )
    } else {
        Controller::new(
            config_arc.read().triangulator.clone(),
            cameras_rx,
            snapshots_rx,
            stream_bcast_tx.clone(),
        )
    };

    let hub_config_for_servers = config_arc.read().clone();
    let recording_state = Arc::new(RecordingState::new(hub_config_for_servers.recording_dir.clone()));
    let hub = Arc::new(HubServer::new(
        cameras_tx,
        snapshots_tx,
        Some(recording_state.clone()),
    ));
    let grpc_server = GrpcServer::new(hub_config_for_servers.grpc.clone(), hub.clone());

    let run_controller = async move {
        match controller.run().await {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow::Error::msg(e.to_string())),
        }
    };
    let run_grpc = grpc_server.run();

    // VRPN server holds C++ state that is !Send, so run it in a dedicated thread with its own runtime.
    let vrpn_config = config_arc.read().vrpn.clone();
    if vrpn_enabled {
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
            let mut vrpn_server = VrpnServer::new(vrpn_config, vrpn_rx);
            if let Err(e) = rt.block_on(vrpn_server.run()) {
                log::error!("VRPN server error: {}", e);
            }
        });
    }

    let run_ws: Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> = if ws_enabled {
        let ws_config = config_arc.read().websocket.clone();
        let ws_server = WebSocketServer::new(
            ws_config,
            stream_bcast_tx.subscribe(),
            Some(hub.clone()),
            Some(recording_state.clone()),
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

    // HTTP MJPEG camera streams: always run so camera feeds are available (e.g. for frontend or other clients).
    let run_http: Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> = {
        let http_config = config_arc.read().http.clone();
        let http_server = HttpServer::new(
            http_config,
            hub.clone(),
            Some(recording_state.clone()),
        );
        Box::pin(async move {
            match http_server.run().await {
                Ok(()) => Ok(()),
                Err(e) => Err(anyhow::Error::msg(e.to_string())),
            }
        })
    };

    let servers = async {
        tokio::try_join!(run_controller, run_grpc, run_ws, run_http)
    };

    let ctrl_c_exit = async {
        let mut last_ctrl_c: Option<Instant> = None;
        loop {
            signal::ctrl_c().await.expect("ctrl_c signal");
            let now = Instant::now();
            let exit = last_ctrl_c
                .map(|t| now.duration_since(t) < CTRL_C_EXIT_WINDOW)
                .unwrap_or(false);
            if exit {
                log::info!("Second Ctrl-C within {:?}, exiting", CTRL_C_EXIT_WINDOW);
                return;
            }
            last_ctrl_c = Some(now);
            if let Some(ref path) = config_path {
                match HubConfig::load_path(path) {
                    Ok(new_config) => {
                        let tri = new_config.triangulator.clone();
                        {
                            let mut guard = config_arc.write();
                            *guard = new_config;
                        }
                        let _ = config_reload_tx.send(tri);
                        log::info!("Config reloaded from {}", path.display());
                    }
                    Err(e) => log::error!("Config reload failed: {}", e),
                }
            } else {
                log::info!("No config file (use --config or POSENET_CONFIG); second Ctrl-C exits");
            }
        }
    };

    tokio::select! {
        res = servers => res,
        _ = ctrl_c_exit => Ok(((), (), (), ())),
    }?;
    log::info!("Hub main end");

    Ok(())
}
