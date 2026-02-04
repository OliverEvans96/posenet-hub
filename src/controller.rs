use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::{sync::broadcast, try_join};

use crate::grpc::proto::{CameraInfo, Snapshot};
use crate::triangulator::{LabeledPoses3D, Triangulator, TriangulatorConfig};

/// Application-level error type. Use `anyhow::Result` at binary boundaries.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub struct TriangulatorInfo {
    cameras_tx: UnboundedSender<CameraInfo>,
    snapshots_tx: UnboundedSender<Snapshot>,
}

pub struct Controller {
    config: TriangulatorConfig,
    cameras_rx: UnboundedReceiver<CameraInfo>,
    snapshots_rx: UnboundedReceiver<Snapshot>,
    poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    triangulators: Arc<RwLock<HashMap<String, TriangulatorInfo>>>,
}

impl Controller {
    pub fn new(
        config: TriangulatorConfig,
        cameras_rx: UnboundedReceiver<CameraInfo>,
        snapshots_rx: UnboundedReceiver<Snapshot>,
        poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    ) -> Self {
        Self {
            config,
            cameras_rx,
            snapshots_rx,
            poses3d_tx,
            triangulators: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn run(self) -> Result<(), BoxError> {
        let (cameras_rx, snapshots_rx) = (self.cameras_rx, self.snapshots_rx);
        let triangulators = self.triangulators;
        let config = self.config;
        let poses3d_tx = self.poses3d_tx;
        let triangulators_for_poses = triangulators.clone();
        let poses_handle = tokio::spawn(async move {
            Self::listen_for_poses(snapshots_rx, triangulators_for_poses).await
        });
        let cameras_handle = tokio::spawn(async move {
            Self::listen_for_cameras(cameras_rx, triangulators, config, poses3d_tx).await
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
            match triangulators.read().get(&group_name) {
                Some(info) => {
                    info.snapshots_tx
                        .send(labeled)
                        .expect("triangulator channel closed.");
                }
                None => {
                    log::warn!("Received a pose with an unregistered group name, discarding..");
                }
            };
        }
    }

    async fn listen_for_cameras(
        mut cameras_rx: UnboundedReceiver<CameraInfo>,
        triangulators: Arc<RwLock<HashMap<String, TriangulatorInfo>>>,
        config: TriangulatorConfig,
        poses3d_tx: broadcast::Sender<LabeledPoses3D>,
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

                let t = Triangulator::new(
                    config.clone(),
                    group_name.clone(),
                    new_cameras_rx,
                    snapshots_rx,
                    poses3d_tx.clone(),
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
