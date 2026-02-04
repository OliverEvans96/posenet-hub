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

    //     pub async fn run(mut self) -> Result<(), BoxError> {
    //         try_join!(self.listen_for_cameras(), self.listen_for_poses())?;
    //         Ok(())
    //     }

    //     async fn listen_for_poses(&mut self) -> Result<(), BoxError> {
    //         loop {
    //             // TODO: Proper error handling
    //             let labeled = self.snapshots_rx.recv().await.expect("no message");
    //             match self.triangulators.read().unwrap().get(
    //                 &labeled
    //                     .which_camera
    //                     .as_ref()
    //                     .map(|id| id.group_name.clone())
    //                     // TODO: What if which_camera is None? (parse, don't validate)
    //                     .unwrap_or_default(),
    //             ) {
    //                 Some(info) => info
    //                     .snapshots_tx
    //                     .send(labeled)
    //                     .expect("triangulator channel closed."),
    //                 None => println!(
    //                     "Warning: Received a pose with an unregistered group name, discarding.."
    //                 ),
    //             };
    //         }
    //     }

    //     async fn listen_for_cameras(&self) -> Result<(), BoxError> {
    //         loop {
    //             // TODO: Proper error handling
    //             let camera = self.cameras_rx.recv().await.expect("no message");
    //             let group_name = camera
    //                 .which_camera
    //                 .as_ref()
    //                 .map(|id| id.group_name.clone())
    //                 // TODO: What if which_camera is None?
    //                 .unwrap_or_default();
    //             let create_group = match self.triangulators.read().unwrap().get(&group_name) {
    //                 Some(info) => {
    //                     info.cameras_tx
    //                         .send(camera.clone())
    //                         .expect("triangulator channel closed.");
    //                     false
    //                 }
    //                 None => true,
    //             };

    //             if create_group {
    //                 println!("New camera group --> {}", group_name.clone());
    //                 let (cameras_tx, cameras_rx) = unbounded_channel::<CameraInfo>();
    //                 let (snapshots_tx, snapshots_rx) = unbounded_channel::<Snapshot>();

    //                 let t = Triangulator::new(
    //                     self.config.clone(),
    //                     group_name.clone(),
    //                     cameras_rx,
    //                     snapshots_rx,
    //                     self.poses3d_tx.clone(),
    //                 );
    //                 let info = TriangulatorInfo {
    //                     cameras_tx,
    //                     snapshots_tx,
    //                 };
    //                 info.cameras_tx
    //                     .send(camera)
    //                     .expect("triangulator channel closed.");

    //                 self.triangulators
    //                     .write()
    //                     .expect("triangulators lock poisoned!")
    //                     .insert(group_name, info);

    //                 tokio::spawn(async move {
    //                     t.run().await.unwrap();
    //                 });
    //             }
    //         }
    //     }
}
