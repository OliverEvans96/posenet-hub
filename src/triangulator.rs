use async_std::channel;
use nalgebra::{Matrix3x4, Matrix3, Point2, Point3};
use std::sync::{Arc, RwLock};
use std::{collections::HashMap, time::Duration};
use std::time::Instant;

use tokio::{sync::broadcast, time::sleep, try_join};

use crate::grpc::proto::{CameraInfo, Pose3D};
use crate::grpc::server::{LabeledPoses2D};
use crate::controller::{BoxError};
use crate::openmvg::openmvg::{triangulate_many};
use crate::utils::transpose_vecvec;

#[derive(Debug, Clone)]
pub struct TriangulatorConfig {
    /// Poses older than this will be ignored
    pub pose_expiration: Duration,
    /// Wait this long before recalculating pose
    pub poll_interval: Duration,
    pub min_cameras: usize,
}

#[derive(Debug, Clone)]
pub struct LabeledPoses3D {
    pub group_name: String,
    pub poses: Vec<Pose3D>,
    pub time: Instant
}

pub struct CameraState {
    pub info: CameraInfo,
    pub matrix: Matrix3x4<f64>,
}

impl Default for TriangulatorConfig {
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
fn collect_points_by_keypoint(poses: Vec<LabeledPoses2D>) -> Option<Vec<Vec<Point2<f64>>>> {
    // x[i][j] is the coorinates of keypoint j as seen from camera i
    let points_grouped_by_camera: Vec<Vec<Point2<f64>>> = poses
        .into_iter()
        .map(|labeled| labeled.poses.into_iter().nth(0).unwrap().into())
        .collect();
    // x[i][j] is the coorinates of keypoint i as seen from camera j
    let points_grouped_by_keypoint = transpose_vecvec(&points_grouped_by_camera);
    Some(points_grouped_by_keypoint)
}

pub fn triangulate_from_poses_and_camera_matrices(
    poses: Vec<LabeledPoses2D>,
    camera_matrices: Vec<Matrix3x4<f64>>,
) -> Vec<Point3<f64>> {
    // Rearrange 2D points to correct order
    let points2d_slice = collect_points_by_keypoint(poses).expect("Error while collecting points");
    // Reconstruct the 3D points
    triangulate_many(&points2d_slice, &camera_matrices)
}


pub struct Triangulator {
    config: TriangulatorConfig,
    group_name: String,
    cameras_rx: channel::Receiver<CameraInfo>,
    poses2d_rx: channel::Receiver<LabeledPoses2D>,
    poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    cameras: Arc<RwLock<HashMap<String, CameraState>>>,
    poses: Arc<RwLock<HashMap<String, LabeledPoses2D>>>,
}

impl Triangulator {
    pub fn new(
        config: TriangulatorConfig,
        group_name: String,
        cameras_rx: channel::Receiver<CameraInfo>,
        poses2d_rx: channel::Receiver<LabeledPoses2D>,
        poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    ) -> Self {
        Self {
            config,
            group_name,
            cameras_rx,
            poses2d_rx,
            poses3d_tx,
            cameras: Arc::new(RwLock::new(HashMap::new())),
            poses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn run(&self) -> Result<(), BoxError> {
        try_join!(self.listen_for_poses(), self.listen_for_cameras(), self.triangulate())?;
        Ok(())
    }
    
    pub async fn triangulate(&self) -> Result<(), BoxError> {
        let mut i = 0;
        let mut count = 0;
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
                self.poses3d_tx.send(LabeledPoses3D{
                    group_name: self.group_name.clone(),
                    poses: vec![pose3d],
                    time: Instant::now() 
                }).unwrap();
                count += 1;
            } else {
                // Otherwise, tell VRPN there are no new poses
                self.poses3d_tx.send(LabeledPoses3D{
                    group_name: self.group_name.clone(),
                    poses: Vec::new(),
                    time: Instant::now() 
                }).unwrap();
            }

            i = i % 500 + 1;
            if i == 1 && count > 0 { 
                println!("Generated 3D poses for {} --> {}", self.group_name.clone() + ".pose0", count);
                count = 0;
            }

            // This controls the VRPN update interval
            sleep(self.config.poll_interval).await;
        }
    }

    async fn listen_for_poses(&self) -> Result<(), BoxError> {
        loop {
            let labeled = self.poses2d_rx.recv().await?;
            self.poses
                .write()
                .expect("poses_hm lock poisoned!")
                .insert(labeled.camera_name.clone(), labeled);
        }
    }

    async fn listen_for_cameras(&self) -> Result<(), BoxError> {
        loop {
            let camera = self.cameras_rx.recv().await?;
            let matrix = self.calculate_camera_matrix(&camera).expect("error in calculate camera matrix");
            let state = CameraState {
                info: camera,
                matrix
            };
            println!("New camera --> {}:{}", state.info.group_name.clone(), state.info.camera_name.clone());
            self.cameras
                .write()
                .expect("cameras_hm lock poisoned!")
                .insert(state.info.camera_name.clone(), state);
        }
    }

    fn get_current_poses(&self) -> Vec<LabeledPoses2D> {
        self.poses
            .read()
            .expect("state lock poisoned!")
            .values()
            .filter(|&p| p.time.elapsed() < self.config.pose_expiration)
            .map(|p| p.clone())
            .collect::<Vec<_>>()
    }


    fn calculate_camera_matrix(&self, info: &CameraInfo) -> Option<Matrix3x4<f64>> {
        let intrinsics = info.intrinsics.as_ref()?;
        let extrinsics = info.extrinsics.as_ref()?;

        let c = &intrinsics.camera_matrix;
        let k = Matrix3::new(c[0],c[1],c[2],c[3],c[4],c[5],c[6],c[7],c[8]);

        let v = &extrinsics.view_matrix;
        let rt = Matrix3x4::new(v[0],v[1],v[2],v[3],v[4],v[5],v[6],v[7],v[8],v[9],v[10],v[11]);

        let p = k * rt;
        Some(p)
    }

    fn get_camera_matrix(&self, name: &str) -> Option<Matrix3x4<f64>> {
        let hm = self.cameras.read().expect("state lock poisoned!");
        let camera = hm.get(name)?;
        Some(camera.matrix)
    }

    fn get_current_cameras(&self, poses: &[LabeledPoses2D]) -> Vec<Matrix3x4<f64>> {
        poses.iter().map(|pose| {
            self.get_camera_matrix(&pose.camera_name)
                .expect("Error while getting camera matrix")
        }).collect()
    }
}