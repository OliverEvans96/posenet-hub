use async_std::channel;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::{sync::broadcast, try_join};

use crate::grpc::proto::{CameraInfo, Snapshot};
use crate::triangulator::{LabeledPoses3D, Triangulator, TriangulatorConfig};

// https://benkay86.github.io/rust-error-tutorial.html
pub type BoxError = std::boxed::Box<
    dyn std::error::Error // must implement Error to satisfy ?
        + std::marker::Send // needed for threads
        + std::marker::Sync, // needed for threads
>;

pub struct TriangulatorInfo {
    cameras_tx: channel::Sender<CameraInfo>,
    snapshots_tx: channel::Sender<Snapshot>,
}

pub struct Controller {
    config: TriangulatorConfig,
    cameras_rx: channel::Receiver<CameraInfo>,
    snapshots_rx: channel::Receiver<Snapshot>,
    poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    triangulators: Arc<RwLock<HashMap<String, TriangulatorInfo>>>,
}

impl Controller {
    pub fn new(
        config: TriangulatorConfig,
        cameras_rx: channel::Receiver<CameraInfo>,
        snapshots_rx: channel::Receiver<Snapshot>,
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
        try_join!(self.listen_for_cameras(), self.listen_for_poses())?;
        Ok(())
    }

    async fn listen_for_poses(&self) -> Result<(), BoxError> {
        loop {
            let labeled = self.snapshots_rx.recv().await?;
            match self.triangulators.read().unwrap().get(
                &labeled
                    .which_camera
                    .as_ref()
                    .map(|id| id.group_name.clone())
                    // TODO: What if which_camera is None? (parse, don't validate)
                    .unwrap_or_default(),
            ) {
                Some(info) => info
                    .snapshots_tx
                    .send(labeled)
                    .await
                    .expect("triangulator channel closed."),
                None => println!(
                    "Warning: Received a pose with an unregistered group name, discarding.."
                ),
            };
        }
    }

    async fn listen_for_cameras(&self) -> Result<(), BoxError> {
        loop {
            let camera = self.cameras_rx.recv().await?;
            let group_name = camera
                .which_camera
                .as_ref()
                .map(|id| id.group_name.clone())
                // TODO: What if which_camera is None?
                .unwrap_or_default();
            let create_group = match self.triangulators.read().unwrap().get(&group_name) {
                Some(info) => {
                    info.cameras_tx
                        .send(camera.clone())
                        .await
                        .expect("triangulator channel closed.");
                    false
                }
                None => true,
            };

            if create_group {
                println!("New camera group --> {}", group_name.clone());
                let (cameras_tx, cameras_rx) = channel::unbounded::<CameraInfo>();
                let (snapshots_tx, snapshots_rx) = channel::unbounded::<Snapshot>();

                let t = Triangulator::new(
                    self.config.clone(),
                    group_name.clone(),
                    cameras_rx,
                    snapshots_rx,
                    self.poses3d_tx.clone(),
                );
                let info = TriangulatorInfo {
                    cameras_tx,
                    snapshots_tx,
                };
                info.cameras_tx
                    .send(camera)
                    .await
                    .expect("triangulator channel closed.");

                self.triangulators
                    .write()
                    .expect("triangulators lock poisoned!")
                    .insert(group_name, info);

                tokio::spawn(async move {
                    t.run().await.unwrap();
                });
            }
        }
    }
}
