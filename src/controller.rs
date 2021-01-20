use async_std::channel;
use nalgebra::{Matrix3x4, Point2, Point3, Rotation};
use openmvg::openmvg::{create_camera_matrix, triangulate_many};
use posenet_vr_hub::openmvg;
use std::sync::{Arc, RwLock};
use std::{collections::HashMap, error::Error, time::Duration};
use tokio::{time::delay_for, try_join};

use crate::grpc::proto::{CameraInfo, Pose3D};
use crate::grpc::server::{LabeledPose2D, NamedCameraInfo};
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
            pose_expiration: Duration::from_millis(500),
            poll_interval: Duration::from_millis(500),
            min_cameras: 2,
        }
    }
}

pub struct Controller {
    cameras_rx: channel::Receiver<NamedCameraInfo>,
    poses2d_rx: channel::Receiver<LabeledPose2D>,
    poses3d_tx: channel::Sender<Pose3D>,
    config: ControllerConfig,
    poses_hm: Arc<RwLock<HashMap<String, LabeledPose2D>>>,
    cameras_hm: Arc<RwLock<HashMap<String, CameraInfo>>>,
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

fn triangulate_from_poses_and_camera_matrices(
    poses: Vec<LabeledPose2D>,
    camera_matrices: Vec<Matrix3x4<f64>>,
) -> Vec<Point3<f64>> {
    // Rearrange 2D points to correct order
    let points2d_slice = collect_points_by_keypoint(poses).expect("Error while collecting points");
    // Reconstruct the 3D points
    triangulate_many(&points2d_slice, &camera_matrices)
}

impl Controller {
    pub fn new(
        cameras_rx: channel::Receiver<NamedCameraInfo>,
        poses2d_rx: channel::Receiver<LabeledPose2D>,
        poses3d_tx: channel::Sender<Pose3D>,
        config: ControllerConfig,
    ) -> Self {
        Self {
            cameras_rx,
            poses2d_rx,
            poses3d_tx,
            config,
            poses_hm: Arc::new(RwLock::new(HashMap::new())),
            cameras_hm: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        // TODO: How to handle failure? exit early? continue?
        try_join!(
            self.listen_for_poses(),
            self.listen_for_cameras(),
            self.periodically_triangulate()
        )?;

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

    async fn periodically_triangulate(&self) -> Result<(), Box<dyn Error>> {
        loop {
            // Get current poses and cameras
            let poses = self.get_current_poses();
            let camera_matrices = self.get_current_cameras(poses.as_ref());
            // See if we have enough cameras to proceed
            if camera_matrices.len() >= self.config.min_cameras {
                // Reconstruct the 3D points
                let points3d = triangulate_from_poses_and_camera_matrices(poses, camera_matrices);
                // Send 3D points to VRPN
                self.poses3d_tx.send(points3d.into()).await?;
            }

            delay_for(self.config.poll_interval).await;
        }
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
        let extrinsics = camera.extrinsics.as_ref()?;
        let euler_angles = extrinsics.orientation.as_ref()?;
        let rotation =
            Rotation::from_euler_angles(euler_angles.roll, euler_angles.pitch, euler_angles.yaw);
        let position = extrinsics.position.as_ref()?;
        let center = Point3::new(position.x, position.y, position.z);
        let p = create_camera_matrix(center, rotation);

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
