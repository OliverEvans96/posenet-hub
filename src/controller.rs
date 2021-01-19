use async_std::channel;
use nalgebra::{Matrix3x4, Point2, Point3, Rotation};
use openmvg::openmvg::{create_camera_matrix, triangulate_many};
use posenet_vr_hub::openmvg;
use std::sync::{Arc, RwLock};
use std::{collections::HashMap, error::Error, time::Duration};
use tokio::{time::delay_for, try_join};

use crate::grpc::proto::{CameraInfo, Pose3D};
use crate::grpc::server::{LabeledPose2D, NamedCameraInfo};

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
fn collect_points_by_keypoint(poses: &[LabeledPose2D]) -> Option<Vec<Vec<Point2<f64>>>> {
    let num_keypoints = 17;
    let num_cameras = poses.len();
    let mut keypoint_vec: Vec<Vec<Point2<f64>>> = (0..num_keypoints)
        .map(|_| {
            (0..num_cameras)
                .map(|_| Point2::<f64>::new(0.0, 0.0))
                .collect()
        })
        .collect();

    for i in 0..num_cameras {
        // TODO: Allow missing keypoints
        // This will fail if anything is missing
        keypoint_vec[0][i].x = poses[i].pose.nose.as_ref()?.x;
        keypoint_vec[0][i].y = poses[i].pose.nose.as_ref()?.y;
        keypoint_vec[1][i].x = poses[i].pose.left_eye.as_ref()?.x;
        keypoint_vec[1][i].y = poses[i].pose.left_eye.as_ref()?.y;
        keypoint_vec[2][i].x = poses[i].pose.right_eye.as_ref()?.x;
        keypoint_vec[2][i].y = poses[i].pose.right_eye.as_ref()?.y;
        keypoint_vec[3][i].x = poses[i].pose.left_ear.as_ref()?.x;
        keypoint_vec[3][i].y = poses[i].pose.left_ear.as_ref()?.y;
        keypoint_vec[4][i].x = poses[i].pose.right_ear.as_ref()?.x;
        keypoint_vec[4][i].y = poses[i].pose.right_ear.as_ref()?.y;
        keypoint_vec[5][i].x = poses[i].pose.left_shoulder.as_ref()?.x;
        keypoint_vec[5][i].y = poses[i].pose.left_shoulder.as_ref()?.y;
        keypoint_vec[6][i].x = poses[i].pose.right_shoulder.as_ref()?.x;
        keypoint_vec[6][i].y = poses[i].pose.right_shoulder.as_ref()?.y;
        keypoint_vec[7][i].x = poses[i].pose.left_elbow.as_ref()?.x;
        keypoint_vec[7][i].y = poses[i].pose.left_elbow.as_ref()?.y;
        keypoint_vec[8][i].x = poses[i].pose.right_elbow.as_ref()?.x;
        keypoint_vec[8][i].y = poses[i].pose.right_elbow.as_ref()?.y;
        keypoint_vec[9][i].x = poses[i].pose.left_wrist.as_ref()?.x;
        keypoint_vec[9][i].y = poses[i].pose.left_wrist.as_ref()?.y;
        keypoint_vec[10][i].x = poses[i].pose.right_wrist.as_ref()?.x;
        keypoint_vec[10][i].y = poses[i].pose.right_wrist.as_ref()?.y;
        keypoint_vec[11][i].x = poses[i].pose.left_hip.as_ref()?.x;
        keypoint_vec[11][i].y = poses[i].pose.left_hip.as_ref()?.y;
        keypoint_vec[12][i].x = poses[i].pose.right_hip.as_ref()?.x;
        keypoint_vec[12][i].y = poses[i].pose.right_hip.as_ref()?.y;
        keypoint_vec[13][i].x = poses[i].pose.left_knee.as_ref()?.x;
        keypoint_vec[13][i].y = poses[i].pose.left_knee.as_ref()?.y;
        keypoint_vec[14][i].x = poses[i].pose.right_knee.as_ref()?.x;
        keypoint_vec[14][i].y = poses[i].pose.right_knee.as_ref()?.y;
        keypoint_vec[15][i].x = poses[i].pose.left_ankle.as_ref()?.x;
        keypoint_vec[15][i].y = poses[i].pose.left_ankle.as_ref()?.y;
        keypoint_vec[16][i].x = poses[i].pose.right_ankle.as_ref()?.x;
        keypoint_vec[16][i].y = poses[i].pose.right_ankle.as_ref()?.y;
    }

    Some(keypoint_vec)
}

fn triangulate_from_poses_and_camera_matrices(
    poses: &[LabeledPose2D],
    camera_matrices: Vec<Matrix3x4<f64>>,
) -> Vec<Point3<f64>> {
    // Rearrange 2D points to correct order
    let points2d =
        collect_points_by_keypoint(poses.as_ref()).expect("Error while collecting points");
    let points2d_slices: Vec<_> = points2d.iter().map(|v| v.as_slice()).collect();
    println!("# Poses: {}", poses.len());
    println!("# Cameras: {}", camera_matrices.len());
    println!(
        "points2d_slices: {} (outer), {} (inner)",
        points2d_slices.len(),
        points2d_slices[0].len(),
    );
    // Reconstruct the 3D points
    triangulate_many(points2d_slices.as_ref(), camera_matrices.as_ref())
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
                let points3d =
                    triangulate_from_poses_and_camera_matrices(poses.as_ref(), camera_matrices);
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
