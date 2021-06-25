use async_std::channel;
use nalgebra::{Matrix3x4, Matrix3, Point2, Point3, Rotation};
use std::sync::{Arc, RwLock};
use std::{collections::HashMap, error::Error, time::Duration};
use tokio::{time::sleep, try_join};

use crate::grpc::proto::{CameraInfo, Pose3D};
use crate::grpc::server::{LabeledPose2D, NamedCameraInfo};
use crate::openmvg::openmvg::{create_camera_matrix, triangulate_many};
use crate::utils::transpose_vecvec;

pub struct ControllerConfig {
    /// Poses older than this will be ignored
    pub pose_expiration: Duration,
    /// Wait this long before recalculating pose
    pub poll_interval: Duration,
    pub min_cameras: usize,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            pose_expiration: Duration::from_millis(100),
            poll_interval: Duration::from_millis(16),
            min_cameras: 2,
        }
    }
}

/// Group associated points by keypoint (projections of same 3d point)
/// and convert from gRPC to nalgebra type
fn collect_points_by_keypoint(poses: Vec<LabeledPose2D>) -> Option<Vec<Vec<Point2<f64>>>> {
    // x[i][j] is the coorinates of keypoint j as seen from camera i
    let points_grouped_by_camera: Vec<Vec<Point2<f64>>> = poses
        .into_iter()
        .map(|labeled| labeled.pose.into())
        .collect();
    // x[i][j] is the coorinates of keypoint i as seen from camera j
    let points_grouped_by_keypoint = transpose_vecvec(&points_grouped_by_camera);
    Some(points_grouped_by_keypoint)
}

pub fn triangulate_from_poses_and_camera_matrices(
    poses: Vec<LabeledPose2D>,
    camera_matrices: Vec<Matrix3x4<f64>>,
) -> Vec<Point3<f64>> {
    // Rearrange 2D points to correct order
    let points2d_slice = collect_points_by_keypoint(poses).expect("Error while collecting points");
    // Reconstruct the 3D points
    triangulate_many(&points2d_slice, &camera_matrices)
}

pub struct Aggregator {
    cameras_rx: channel::Receiver<NamedCameraInfo>,
    poses2d_rx: channel::Receiver<LabeledPose2D>,
    cameras_hm: Arc<RwLock<HashMap<String, CameraInfo>>>,
    poses_hm: Arc<RwLock<HashMap<String, LabeledPose2D>>>,
}

impl Aggregator {
    async fn run(&self) -> Result<(), Box<dyn Error>> {
        try_join!(self.listen_for_poses(), self.listen_for_cameras())?;

        Ok(())
    }

    async fn listen_for_poses(&self) -> Result<(), Box<dyn Error>> {
        loop {
            let labeled = self.poses2d_rx.recv().await?;
            self.poses_hm
                .write()
                .expect("poses_hm lock poisoned!")
                .insert(labeled.name.clone(), labeled);
        }
    }

    async fn listen_for_cameras(&self) -> Result<(), Box<dyn Error>> {
        loop {
            let camera = self.cameras_rx.recv().await?;
            self.cameras_hm
                .write()
                .expect("cameras_hm lock poisoned!")
                .insert(camera.name, camera.info);
        }
    }
}

pub struct Triangulator {
    poses3d_tx: channel::Sender<Option<Pose3D>>,
    cameras_hm: Arc<RwLock<HashMap<String, CameraInfo>>>,
    poses_hm: Arc<RwLock<HashMap<String, LabeledPose2D>>>,
    config: ControllerConfig,
}

impl Triangulator {
    async fn run(&self) -> Result<(), Box<dyn Error>> {
        loop {
            // Get current poses and cameras
            let poses = self.get_current_poses();
            let camera_matrices = self.get_current_cameras(poses.as_ref());

            // See if we have enough cameras to proceed
            if camera_matrices.len() >= self.config.min_cameras {
                // Reconstruct the 3D points
                let points3d = triangulate_from_poses_and_camera_matrices(poses, camera_matrices);
                let pose3d = points3d.into();
                // Send 3D points to VRPN
                self.poses3d_tx.send(Some(pose3d)).await?;
            } else {
                // Otherwise, tell VRPN there are no new poses
                self.poses3d_tx.send(None).await?;
            }

            // This controls the VRPN update interval
            sleep(self.config.poll_interval).await;
        }
    }

    async fn publish_pose3d(&self, pose: Pose3D) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn get_current_poses(&self) -> Vec<LabeledPose2D> {
        self.poses_hm
            .read()
            .expect("poses_hm lock poisoned!")
            .values()
            .filter(|&v| v.time.elapsed() < self.config.pose_expiration)
            .map(|v| v.clone())
            .collect::<Vec<_>>()
    }

    fn get_camera_matrix(&self, name: &str) -> Option<Matrix3x4<f64>> {
        let hm = self.cameras_hm.read().expect("cameras_hm lock poisoned!");
        let camera = hm.get(name)?;
        let intrinsics = camera.intrinsics.as_ref()?;
        let extrinsics = camera.extrinsics.as_ref()?;

        let c = &intrinsics.camera_matrix;
        let k = Matrix3::new(c[0],c[1],c[2],c[3],c[4],c[5],c[6],c[7],c[8]);

        let v = &extrinsics.view_matrix;
        let rt = Matrix3x4::new(v[0],v[1],v[2],v[3],v[4],v[5],v[6],v[7],v[8],v[9],v[10],v[11]);

        let p = k * rt;
        Some(p)
    }

    fn get_current_cameras(&self, poses: &[LabeledPose2D]) -> Vec<Matrix3x4<f64>> {
        poses
            .iter()
            .map(|pose| {
                self.get_camera_matrix(&pose.name)
                    .expect("Error while getting camera matrix")
            })
            .collect()
    }
}

pub struct Controller {
    config: ControllerConfig,
    cameras_rx: channel::Receiver<NamedCameraInfo>,
    poses2d_rx: channel::Receiver<LabeledPose2D>,
    poses3d_tx: channel::Sender<Option<Pose3D>>,
}

impl Controller {
    pub fn new(
        config: ControllerConfig,
        cameras_rx: channel::Receiver<NamedCameraInfo>,
        poses2d_rx: channel::Receiver<LabeledPose2D>,
        poses3d_tx: channel::Sender<Option<Pose3D>>,
    ) -> Self {
        Self {
            config,
            cameras_rx,
            poses2d_rx,
            poses3d_tx,
        }
    }

    pub async fn run(self) -> Result<(), Box<dyn Error>> {
        let poses_hm = Arc::new(RwLock::new(HashMap::new()));
        let cameras_hm = Arc::new(RwLock::new(HashMap::new()));

        let triangulator = Triangulator {
            config: self.config,
            poses3d_tx: self.poses3d_tx,
            cameras_hm: Arc::clone(&cameras_hm),
            poses_hm: Arc::clone(&poses_hm),
        };

        let aggregator = Aggregator {
            cameras_rx: self.cameras_rx,
            poses2d_rx: self.poses2d_rx,
            cameras_hm,
            poses_hm,
        };

        // TODO: How to handle failure? exit early? continue?
        let result = try_join!(triangulator.run(), aggregator.run());
        match result {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }
}
