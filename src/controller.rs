use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::{sync::broadcast, try_join};

use crate::config::HubConfig;
use crate::grpc::proto::{CameraInfo, Snapshot};
use crate::triangulator::{PoseStreamUpdate, Triangulator, TriangulatorConfig};

/// Application-level error type. Use `anyhow::Result` at binary boundaries.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub struct TriangulatorInfo {
    cameras_tx: UnboundedSender<CameraInfo>,
    snapshots_tx: UnboundedSender<Snapshot>,
}

pub struct Controller {
    /// Shared config; read when spawning new triangulators. If None, use initial_config for new groups only.
    config_arc: Option<Arc<RwLock<HubConfig>>>,
    initial_config: TriangulatorConfig,
    config_reload_tx: Option<broadcast::Sender<TriangulatorConfig>>,
    cameras_rx: UnboundedReceiver<CameraInfo>,
    snapshots_rx: UnboundedReceiver<Snapshot>,
    stream_tx: broadcast::Sender<PoseStreamUpdate>,
    triangulators: Arc<RwLock<HashMap<String, TriangulatorInfo>>>,
}

impl Controller {
    /// Create a controller with fixed config (no reload). New triangulators use initial_config.
    pub fn new(
        config: TriangulatorConfig,
        cameras_rx: UnboundedReceiver<CameraInfo>,
        snapshots_rx: UnboundedReceiver<Snapshot>,
        stream_tx: broadcast::Sender<PoseStreamUpdate>,
    ) -> Self {
        Self {
            config_arc: None,
            initial_config: config,
            config_reload_tx: None,
            cameras_rx,
            snapshots_rx,
            stream_tx,
            triangulators: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a controller that reads config from shared Arc and sends reloads to triangulators.
    pub fn new_with_reload(
        config_arc: Arc<RwLock<HubConfig>>,
        config_reload_tx: broadcast::Sender<TriangulatorConfig>,
        cameras_rx: UnboundedReceiver<CameraInfo>,
        snapshots_rx: UnboundedReceiver<Snapshot>,
        stream_tx: broadcast::Sender<PoseStreamUpdate>,
    ) -> Self {
        let initial_config = config_arc.read().triangulator.clone();
        Self {
            config_arc: Some(config_arc),
            initial_config,
            config_reload_tx: Some(config_reload_tx),
            cameras_rx,
            snapshots_rx,
            stream_tx,
            triangulators: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn run(self) -> Result<(), BoxError> {
        let (cameras_rx, snapshots_rx) = (self.cameras_rx, self.snapshots_rx);
        let triangulators = self.triangulators;
        let config_arc = self.config_arc;
        let initial_config = self.initial_config;
        let config_reload_tx = self.config_reload_tx;
        let stream_tx = self.stream_tx;
        let triangulators_for_poses = triangulators.clone();
        let poses_handle = tokio::spawn(async move {
            Self::listen_for_poses(snapshots_rx, triangulators_for_poses).await
        });
        let cameras_handle = tokio::spawn(async move {
            Self::listen_for_cameras(
                cameras_rx,
                triangulators,
                config_arc,
                initial_config,
                config_reload_tx,
                stream_tx,
            )
            .await
        });
        let (poses_res, cameras_res) =
            try_join!(poses_handle, cameras_handle).map_err(|e| Box::new(e) as BoxError)?;
        poses_res?;
        cameras_res?;
        Ok(())
    }

    async fn listen_for_poses(
        mut snapshots_rx: UnboundedReceiver<Snapshot>,
        triangulators: Arc<RwLock<HashMap<String, TriangulatorInfo>>>,
    ) -> Result<(), BoxError> {
        loop {
            let labeled = snapshots_rx.recv().await.expect("no message");
            let group_name = labeled
                .which_camera
                .as_ref()
                .map(|id| id.group_name.clone())
                .unwrap_or_default();
            let num_poses = labeled.poses.len();
            let camera_label = labeled
                .which_camera
                .as_ref()
                .map(|id| id.camera_name.as_str())
                .unwrap_or("?");
            match triangulators.read().get(&group_name) {
                Some(info) => {
                    log::debug!(
                        "Controller: snapshot {}:{} ({} pose(s)) -> triangulator",
                        group_name,
                        camera_label,
                        num_poses
                    );
                    info.snapshots_tx
                        .send(labeled)
                        .expect("triangulator channel closed.");
                }
                None => {
                    log::warn!(
                        "Controller: snapshot for unregistered group '{}', discarding",
                        group_name
                    );
                }
            };
        }
    }

    async fn listen_for_cameras(
        mut cameras_rx: UnboundedReceiver<CameraInfo>,
        triangulators: Arc<RwLock<HashMap<String, TriangulatorInfo>>>,
        config_arc: Option<Arc<RwLock<HubConfig>>>,
        initial_config: TriangulatorConfig,
        config_reload_tx: Option<broadcast::Sender<TriangulatorConfig>>,
        stream_tx: broadcast::Sender<PoseStreamUpdate>,
    ) -> Result<(), BoxError> {
        loop {
            let camera = cameras_rx.recv().await.expect("no message");
            let group_name = camera
                .which_camera
                .as_ref()
                .map(|id| id.group_name.clone())
                .unwrap_or_default();
            let create_group = match triangulators.read().get(&group_name) {
                Some(info) => {
                    info.cameras_tx
                        .send(camera.clone())
                        .expect("triangulator channel closed.");
                    false
                }
                None => true,
            };

            if create_group {
                log::info!("New camera group --> {}", group_name);
                let (cameras_tx, new_cameras_rx) = unbounded_channel::<CameraInfo>();
                let (snapshots_tx, snapshots_rx) = unbounded_channel::<Snapshot>();

                let config = config_arc
                    .as_ref()
                    .map(|a| a.read().triangulator.clone())
                    .unwrap_or_else(|| initial_config.clone());
                let config_rx = config_reload_tx.as_ref().map(|tx| tx.subscribe());

                let t = Triangulator::new_with_config_reload(
                    config,
                    config_rx,
                    group_name.clone(),
                    new_cameras_rx,
                    snapshots_rx,
                    stream_tx.clone(),
                );
                let info = TriangulatorInfo {
                    cameras_tx,
                    snapshots_tx,
                };
                info.cameras_tx
                    .send(camera)
                    .expect("triangulator channel closed.");

                triangulators.write().insert(group_name, info);

                tokio::spawn(async move {
                    if let Err(e) = t.run().await {
                        log::error!("Triangulator run error: {}", e);
                    }
                });
            }
        }
    }
}
