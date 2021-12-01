use async_std::channel;
use nalgebra::{Matrix3, Matrix3x4, Point2, Point3};
use std::cmp;
use std::convert::{TryFrom, TryInto};
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime};
use std::{collections::HashMap, time::Duration};
use thiserror::Error;
use tokio::{sync::broadcast, time::sleep, try_join};

use crate::controller::BoxError;
use crate::grpc::proto;
use crate::grpc::proto::{CameraInfo, Pose2D, Pose3D, SPoint2, SPoint3};
use crate::openmvg::openmvg::triangulate_many;
use crate::utils::transpose_vecvec;
use crate::errors::{MissingField,CalculationError};

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
    pub time: Instant,
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
/// and convert from gRPC to (nalgebra type, score value)
fn collect_points_by_keypoint(poses: Vec<Pose2D>) -> Vec<Vec<SPoint2>> {
    // x[i][j] is the coorinates of keypoint j as seen from camera i
    let points_grouped_by_camera: Vec<Vec<SPoint2>> = poses
        .into_iter()
        .map(|pose| {
            let (points, score) = pose.into();
            points
        })
        .collect();
    // x[i][j] is the coorinates of keypoint i as seen from camera j
    let points_grouped_by_keypoint = transpose_vecvec(&points_grouped_by_camera);
    points_grouped_by_keypoint
}

pub fn calculate_camera_matrix(
    calibration: &proto::CalibrationParameters,
) -> Result<Matrix3x4<f64>, CalculationError> {
    let intrinsics =
        calibration
            .intrinsics
            .as_ref()
            .ok_or(CalculationError::CameraMatrixFailed(
                MissingField::Intrinsics,
            ))?;
    let extrinsics =
        calibration
            .extrinsics
            .as_ref()
            .ok_or(CalculationError::CameraMatrixFailed(
                MissingField::Extrinsics,
            ))?;

    let c = &intrinsics.camera_matrix;
    let k = Matrix3::new(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8]);

    let v = &extrinsics.view_matrix;
    let rt = Matrix3x4::new(
        v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11],
    );

    let p = k * rt;
    Ok(p)
}

pub fn triangulate_from_poses_and_camera_matrices(
    poses: Vec<Pose2D>,
    camera_matrices: &[Matrix3x4<f64>],
) -> Pose3D {
    // Aggregate 2D pose scores by minimum, TODO better way?
    let pose_score = poses
        .iter()
        .fold(1.0, |total, pose| f64::min(total, pose.score));

    // Rearrange 2D points grouped by keypoint
    let keypoints = collect_points_by_keypoint(poses);

    // Aggregate 2D point scores by minimum, TODO better way?
    let keypoint_scores: Vec<f64> = keypoints
        .iter()
        .map(|ks| {
            ks.iter()
                .fold(1.0, |total, (p, score)| f64::min(total, *score))
        })
        .collect();

    // Reconstruct the 3D points
    let points2d: Vec<Vec<Point2<f64>>> = keypoints
        .into_iter()
        .map(|ks| ks.into_iter().map(|(p, s)| p).collect())
        .collect();
    let points3d = triangulate_many(&points2d, camera_matrices);

    // Pose3D from 3D points
    let scored_points3d = points3d
        .into_iter()
        .zip(keypoint_scores.into_iter())
        .collect();
    (scored_points3d, pose_score).into()
}

// pub fn score_from_poses(poses: Vec<Pose2D>, pose3d: Pose3D)

pub struct Triangulator {
    config: TriangulatorConfig,
    group_name: String,
    cameras_rx: channel::Receiver<CameraInfo>,
    snapshots_rx: channel::Receiver<proto::Snapshot>,
    poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    cameras: Arc<RwLock<HashMap<String, CameraState>>>,
    poses: Arc<RwLock<HashMap<String, proto::Snapshot>>>,
}

impl Triangulator {
    pub fn new(
        config: TriangulatorConfig,
        group_name: String,
        cameras_rx: channel::Receiver<CameraInfo>,
        snapshots_rx: channel::Receiver<proto::Snapshot>,
        poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    ) -> Self {
        Self {
            config,
            group_name,
            cameras_rx,
            snapshots_rx,
            poses3d_tx,
            cameras: Arc::new(RwLock::new(HashMap::new())),
            poses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn run(&self) -> Result<(), BoxError> {
        try_join!(
            self.listen_for_poses(),
            self.listen_for_cameras(),
            self.triangulate()
        )?;
        Ok(())
    }

    pub async fn triangulate(&self) -> Result<(), BoxError> {
        let mut i: i32 = 0;
        let mut count: i32 = 0;
        loop {
            // Get current poses
            let current = self.get_current_snapshot();

            // Group by user
            let users = self.group_poses_and_cameras_by_user(current);
            for (i, (poses, camera_matrices)) in users.into_iter().enumerate() {
                // See if we have enough cameras to proceed
                if camera_matrices.len() >= self.config.min_cameras {
                    // Reconstruct the 3D points
                    let points3d =
                        triangulate_from_poses_and_camera_matrices(poses, &camera_matrices);
                    let pose3d = points3d.into();
                    // let scored_pose3d = score_from_poses(poses, pose3d);

                    // Send 3D points to VRPN
                    self.poses3d_tx
                        .send(LabeledPoses3D {
                            group_name: self.group_name.clone(),
                            poses: vec![pose3d],
                            time: Instant::now(),
                        })
                        .unwrap();
                    count += 1;
                } else {
                    // Otherwise, tell VRPN there are no new poses
                    self.poses3d_tx
                        .send(LabeledPoses3D {
                            group_name: self.group_name.clone(),
                            poses: Vec::new(),
                            time: Instant::now(),
                        })
                        .unwrap();
                }
            }

            i = i % 100 + 1;
            if i == 1 && count > 0 {
                println!(
                    "Generated 3D pose count for {} --> {}",
                    self.group_name.clone() + ".pose0",
                    count
                );
                count = 0;
            }

            // This controls the VRPN update interval
            sleep(self.config.poll_interval).await;
        }
    }

    async fn listen_for_poses(&self) -> Result<(), BoxError> {
        loop {
            let snapshot = self.snapshots_rx.recv().await?;
            let camera_name = snapshot
                .which_camera
                .clone()
                .map(|which_camera| which_camera.camera_name)
                .ok_or(MissingField::CameraName)?;
            self.poses
                .write()
                .expect("poses_hm lock poisoned!")
                .insert(camera_name, snapshot);
        }
    }

    async fn listen_for_cameras(&self) -> Result<(), BoxError> {
        loop {
            let camera = self.cameras_rx.recv().await?;
            let camera_name = camera
                .which_camera
                .as_ref()
                .map(|which_camera| which_camera.camera_name.clone())
                .ok_or(MissingField::CameraName)?;
            let group_name = camera
                .which_camera
                .as_ref()
                .map(|which_camera| which_camera.group_name.clone())
                .ok_or(MissingField::GroupName)?;
            let calibration = camera
                .calibration
                .clone()
                .ok_or(MissingField::Calibration)?;
            let matrix = calculate_camera_matrix(&calibration)?;
            let state = CameraState {
                info: camera,
                matrix,
            };
            println!(
                "New camera --> {}:{}",
                group_name.clone(),
                camera_name.clone()
            );
            self.cameras
                .write()
                .expect("cameras_hm lock poisoned!")
                .insert(camera_name.clone(), state);
        }
    }

    fn get_current_snapshot(&self) -> Vec<proto::Snapshot> {
        self.poses
            .read()
            .expect("state lock poisoned!")
            .values()
            .filter_map(|snapshot| {
                // If the snapshot has a timestamp and we can parse it, make sure it isn't expired.
                // If we can't parse the timestamp, ignore this snapshot.
                let timestamp = snapshot.timestamp?;
                let system_time = SystemTime::try_from(timestamp).ok()?;
                let elapsed = system_time.elapsed().ok()?;

                if elapsed < self.config.pose_expiration {
                    Some(snapshot.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn get_pose_for_user(&self, pose: &proto::Snapshot, user_id: usize) -> Option<Pose2D> {
        // TODO identify user somehow, so that poses from different cameras can be grouped
        // delta from previous frames? or use image data somehow, facial recognition?
        // for now just returned in order sent from client, may be glitchy for multiple tracked users
        Some(pose.poses[user_id].clone()) //.iter().nth(user_id)
    }

    fn group_poses_and_cameras_by_user(
        &self,
        snapshots: Vec<proto::Snapshot>,
    ) -> Vec<(Vec<Pose2D>, Vec<Matrix3x4<f64>>)> {
        let max_users = 1; // limit to 1 user for now
        (0..max_users)
            .map(|id| {
                // TODO handle missing snapshots, and remove corresponding camera matrix
                let user_snapshots = (&snapshots)
                    .into_iter()
                    .map(|labeled| self.get_pose_for_user(labeled, id).unwrap())
                    .collect();
                let camera_matrices = self.get_cameras_for_snapshots(&snapshots);
                (user_snapshots, camera_matrices)
            })
            .collect()
    }

    fn get_camera_matrix(&self, name: &str) -> Option<Matrix3x4<f64>> {
        let hm = self.cameras.read().expect("state lock poisoned!");
        let camera = hm.get(name)?;
        Some(camera.matrix)
    }

    fn get_cameras_for_snapshots(&self, snapshots: &[proto::Snapshot]) -> Vec<Matrix3x4<f64>> {
        snapshots
            .iter()
            .map(|snapshot| {
                let camera_name = snapshot
                    .which_camera
                    .as_ref()
                    .map(|which_camera| which_camera.camera_name.as_ref())
                    .ok_or(MissingField::CameraName)
                    .unwrap();
                self.get_camera_matrix(camera_name)
                    .expect("Error while getting camera matrix")
            })
            .collect()
    }
}
