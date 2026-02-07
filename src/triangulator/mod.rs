use nalgebra::{Matrix3, Matrix3x4, Point2, Point3, Vector3};
use parking_lot::RwLock;
use rayon::iter::IntoParallelRefIterator;
use rayon::prelude::*;
use std::convert::TryInto;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::{sync::broadcast, time::sleep, try_join};

use crate::errors::{CalculationError, HubError, MissingField};
use crate::grpc::proto::{
    pose2d_to_spoints_partial, pose3d_from_partial, CalibrationParameters, CameraInfo, Pose2D,
    Pose3D, SPoint2, SPoint3, Snapshot,
};
use crate::openmvg::openmvg::{triangulate, triangulate_many};
use crate::utils::transpose_vecvec;

mod pose_matching;
mod pose_smoothing;
pub use pose_matching::{group_poses_by_index, match_poses, triangulate_matched_groups, MatchedGroup};
pub use pose_smoothing::{smooth_snapshots, Camera2DTracks, SmoothingConfig};

/// Default reprojection threshold (pixels) for pose matching; pairs above this are rejected.
pub const POSE_MATCHING_REPROJECTION_THRESHOLD_PX: f64 = 50.0;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone)]
pub struct TriangulatorConfig {
    /// Poses older than this will be ignored
    pub pose_expiration: Duration,
    /// Wait this long before recalculating pose
    pub poll_interval: Duration,
    pub min_cameras: usize,
    /// Reprojection threshold (px) for pose matching; if None, use POSE_MATCHING_REPROJECTION_THRESHOLD_PX.
    pub pose_matching_threshold_px: Option<f64>,
    /// 2D smoothing config; if None, use SmoothingConfig::default().
    pub smoothing: Option<SmoothingConfig>,
}

#[derive(Debug, Clone)]
pub struct LabeledPoses3D {
    pub group_name: String,
    pub poses: Vec<Pose3D>,
    pub time: Instant,
}

/// Per-camera 2D pose data for streaming to clients (e.g. camera view panels).
#[derive(Debug, Clone)]
pub struct CameraView {
    pub camera_name: String,
    pub poses: Vec<Pose2D>,
}

/// Minimal camera model for frontend visualization (position + orientation basis).
#[derive(Debug, Clone)]
pub struct CameraModel {
    pub camera_name: String,
    /// Intrinsics (in pixels).
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    /// Estimated image size in pixels (may be derived from principal point).
    pub width_px: f64,
    pub height_px: f64,
    /// Camera center in world coordinates.
    pub position: Point3<f64>,
    /// Unit vectors in world coordinates describing camera orientation.
    pub right: Vector3<f64>,
    pub up: Vector3<f64>,
    pub forward: Vector3<f64>,
}

/// Combined 3D poses and per-camera 2D views sent each triangulator tick.
#[derive(Debug, Clone)]
pub struct PoseStreamUpdate {
    pub labeled_poses: LabeledPoses3D,
    pub camera_views: Vec<CameraView>,
    pub cameras: Vec<CameraModel>,
}

#[derive(Clone)]
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
            pose_matching_threshold_px: None,
            smoothing: None,
        }
    }
}

/// Group associated points by keypoint (projections of same 3d point)
/// and convert from gRPC to (nalgebra type, score value)
fn collect_points_by_keypoint(poses: Vec<Pose2D>) -> Result<Vec<Vec<SPoint2>>, HubError> {
    let points_grouped_by_camera: Vec<Vec<SPoint2>> = poses
        .into_iter()
        .map(|pose| -> Result<Vec<SPoint2>, MissingField> {
            let (points, _score) = pose.try_into()?;
            Ok(points)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let points_grouped_by_keypoint = transpose_vecvec(&points_grouped_by_camera)?;
    Ok(points_grouped_by_keypoint)
}

/// Group 2D observations by keypoint for partial poses (allows missing keypoints per camera).
/// Returns, for each keypoint index 0..17, a list of (camera_index, SPoint2) for cameras that have that keypoint.
fn collect_points_by_keypoint_partial(poses: &[Pose2D]) -> Vec<Vec<(usize, SPoint2)>> {
    let partials: Vec<([Option<SPoint2>; 17], f64)> = poses
        .iter()
        .map(|p| pose2d_to_spoints_partial(p))
        .collect();
    (0..17)
        .map(|k| {
            partials
                .iter()
                .enumerate()
                .filter_map(|(cam_idx, (opts, _))| opts[k].as_ref().map(|s| (cam_idx, s.clone())))
                .collect()
        })
        .collect()
}

pub fn calculate_camera_matrix(
    calibration: &CalibrationParameters,
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

/// Build a `CameraModel` from `CameraInfo` calibration extrinsics.
///
/// `extrinsics.view_matrix` is treated as a 3x4 row-major matrix `[R|t]` that maps world → camera:
/// \( x_c = R x_w + t \). Camera center is \( C = -R^T t \).
/// World convention: up = +Z; camera right/up/forward are in world frame.
fn camera_model_from_info(camera: &CameraInfo) -> Option<CameraModel> {
    let which = camera.which_camera.as_ref()?;
    let calibration = camera.calibration.as_ref()?;
    let intrinsics = calibration.intrinsics.as_ref()?;
    let extrinsics = calibration.extrinsics.as_ref()?;
    let v = &extrinsics.view_matrix;
    if v.len() < 12 {
        return None;
    }

    // Row-major 3x4 [R|t]
    let r00 = v[0];
    let r01 = v[1];
    let r02 = v[2];
    let t0 = v[3];
    let r10 = v[4];
    let r11 = v[5];
    let r12 = v[6];
    let t1 = v[7];
    let r20 = v[8];
    let r21 = v[9];
    let r22 = v[10];
    let t2 = v[11];

    let r = Matrix3::new(r00, r01, r02, r10, r11, r12, r20, r21, r22);
    let t = Vector3::new(t0, t1, t2);

    let rt = r.transpose();
    let c = -(rt * t);

    let right = (rt * Vector3::new(1.0, 0.0, 0.0)).normalize();
    let up = (rt * Vector3::new(0.0, 1.0, 0.0)).normalize();
    let forward = (rt * Vector3::new(0.0, 0.0, 1.0)).normalize();

    let k = &intrinsics.camera_matrix;
    if k.len() < 9 {
        return None;
    }
    let fx = k[0];
    let fy = k[4];
    let cx = k[2];
    let cy = k[5];
    // If we don't know resolution, assume principal point is near the center.
    let width_px = if cx > 0.0 { 2.0 * cx } else { 640.0 };
    let height_px = if cy > 0.0 { 2.0 * cy } else { 480.0 };

    Some(CameraModel {
        camera_name: which.camera_name.clone(),
        fx,
        fy,
        cx,
        cy,
        width_px,
        height_px,
        position: Point3::new(c.x, c.y, c.z),
        right,
        up,
        forward,
    })
}

pub fn triangulate_from_poses_and_camera_matrices(
    poses: Vec<Pose2D>,
    camera_matrices: &[Matrix3x4<f64>],
) -> Result<Pose3D, HubError> {
    let pose_score = poses
        .iter()
        .fold(1.0, |total, pose| f64::min(total, pose.score));

    let keypoints = collect_points_by_keypoint(poses)?;

    let keypoint_scores: Vec<f64> = keypoints
        .iter()
        .map(|ks| {
            ks.iter()
                .fold(1.0, |total, (_p, score)| f64::min(total, *score))
        })
        .collect();

    let points2d: Vec<Vec<Point2<f64>>> = keypoints
        .into_iter()
        .map(|ks| ks.into_iter().map(|(p, _s)| p).collect())
        .collect();
    let points3d = triangulate_many(&points2d, camera_matrices)?;

    let scored_points3d = points3d
        .into_iter()
        .zip(keypoint_scores.into_iter())
        .collect();
    Ok((scored_points3d, pose_score).into())
}

/// Triangulate from 2D poses that may have missing keypoints (partial poses).
/// For each keypoint we use only cameras that have that keypoint; if fewer than 2, that 3D keypoint is None.
/// Single-keypoint triangulation failures (e.g. degenerate) also yield None for that keypoint.
pub fn triangulate_from_poses_and_camera_matrices_partial(
    poses: &[Pose2D],
    camera_matrices: &[Matrix3x4<f64>],
) -> Result<Pose3D, HubError> {
    let pose_score = poses
        .iter()
        .fold(1.0, |total, pose| f64::min(total, pose.score));

    let keypoints_per_index = collect_points_by_keypoint_partial(poses);

    let mut keypoints3d: [Option<SPoint3>; 17] = [None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None];

    for (k, observations) in keypoints_per_index.iter().enumerate() {
        if observations.len() < 2 {
            continue;
        }
        let points2d: Vec<Point2<f64>> = observations.iter().map(|(_, (p, _))| *p).collect();
        let matrices: Vec<Matrix3x4<f64>> = observations
            .iter()
            .map(|(i, _)| camera_matrices[*i])
            .collect();
        match triangulate(&points2d, &matrices) {
            Ok(p3) => {
                let score = observations
                    .iter()
                    .fold(1.0, |acc, (_, (_, s))| f64::min(acc, *s));
                keypoints3d[k] = Some((p3, score));
            }
            Err(_) => {}
        }
    }

    Ok(pose3d_from_partial(keypoints3d, pose_score))
}

// pub fn score_from_poses(poses: Vec<Pose2D>, pose3d: Pose3D)

pub struct Triangulator {
    config: TriangulatorConfig,
    /// When Some, triangulate_loop will apply config updates (e.g. from Ctrl-C reload).
    config_rx: Option<broadcast::Receiver<TriangulatorConfig>>,
    group_name: String,
    cameras_rx: UnboundedReceiver<CameraInfo>,
    snapshots_rx: UnboundedReceiver<Snapshot>,
    stream_tx: broadcast::Sender<PoseStreamUpdate>,
    cameras: Arc<RwLock<HashMap<String, CameraState>>>,
    poses: Arc<RwLock<HashMap<String, Snapshot>>>,
}

impl Triangulator {
    pub fn new(
        config: TriangulatorConfig,
        group_name: String,
        cameras_rx: UnboundedReceiver<CameraInfo>,
        snapshots_rx: UnboundedReceiver<Snapshot>,
        stream_tx: broadcast::Sender<PoseStreamUpdate>,
    ) -> Self {
        Self::new_with_config_reload(config, None, group_name, cameras_rx, snapshots_rx, stream_tx)
    }

    /// Same as new but with an optional receiver for config reloads (Ctrl-C).
    pub fn new_with_config_reload(
        config: TriangulatorConfig,
        config_rx: Option<broadcast::Receiver<TriangulatorConfig>>,
        group_name: String,
        cameras_rx: UnboundedReceiver<CameraInfo>,
        snapshots_rx: UnboundedReceiver<Snapshot>,
        stream_tx: broadcast::Sender<PoseStreamUpdate>,
    ) -> Self {
        Self {
            config,
            config_rx,
            group_name,
            cameras_rx,
            snapshots_rx,
            stream_tx,
            cameras: Arc::new(RwLock::new(HashMap::new())),
            poses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Run the triangulator: spawn listen_for_poses, listen_for_cameras, and triangulate_loop; join until one errors.
    pub async fn run(self) -> Result<(), BoxError> {
        let config = self.config.clone();
        let config_rx = self.config_rx;
        let group_name = self.group_name.clone();
        let stream_tx = self.stream_tx.clone();
        let cameras = self.cameras.clone();
        let poses = self.poses.clone();
        let snapshots_rx = self.snapshots_rx;
        let cameras_rx = self.cameras_rx;

        let poses_for_tri = poses.clone();
        let cameras_for_tri = cameras.clone();

        let poses_handle =
            tokio::spawn(async move { Self::listen_for_poses(snapshots_rx, poses).await });
        let group_name_for_cameras = group_name.clone();
        let stream_tx_for_cameras = stream_tx.clone();
        let cameras_handle = tokio::spawn(async move {
            Self::listen_for_cameras(
                cameras_rx,
                cameras,
                group_name_for_cameras,
                stream_tx_for_cameras,
            )
            .await
        });
        let triangulate_handle = tokio::spawn(async move {
            Self::triangulate_loop(
                config,
                config_rx,
                group_name.clone(),
                poses_for_tri,
                cameras_for_tri,
                stream_tx,
            )
            .await
        });

        let (a, b, c) = try_join!(poses_handle, cameras_handle, triangulate_handle)
            .map_err(|e| Box::new(e) as BoxError)?;
        a?;
        b?;
        c?;
        Ok(())
    }

    async fn listen_for_poses(
        mut snapshots_rx: UnboundedReceiver<Snapshot>,
        poses: Arc<RwLock<HashMap<String, Snapshot>>>,
    ) -> Result<(), BoxError> {
        loop {
            let snapshot = match snapshots_rx.recv().await {
                Some(s) => s,
                None => return Ok(()),
            };
            let camera_name = match snapshot.which_camera.as_ref() {
                Some(id) => id.camera_name.clone(),
                None => {
                    log::warn!("Snapshot missing which_camera, discarding");
                    continue;
                }
            };
            let num_poses = snapshot.poses.len();
            log::debug!(
                "Triangulator: snapshot from {} ({} pose(s))",
                camera_name,
                num_poses
            );
            poses.write().insert(camera_name, snapshot);
        }
    }

    async fn listen_for_cameras(
        mut cameras_rx: UnboundedReceiver<CameraInfo>,
        cameras: Arc<RwLock<HashMap<String, CameraState>>>,
        group_name: String,
        stream_tx: broadcast::Sender<PoseStreamUpdate>,
    ) -> Result<(), BoxError> {
        loop {
            let camera = match cameras_rx.recv().await {
                Some(c) => c,
                None => return Ok(()),
            };
            let camera_name = camera
                .which_camera
                .as_ref()
                .map(|id| id.camera_name.clone())
                .unwrap_or_default();
            if camera_name.is_empty() {
                log::warn!("CameraInfo missing camera_name, discarding");
                continue;
            }
            let calibration = match camera.calibration.as_ref() {
                Some(cal) => cal.clone(),
                None => {
                    log::warn!("CameraInfo missing calibration, discarding");
                    continue;
                }
            };
            let matrix = match calculate_camera_matrix(&calibration) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("calculate_camera_matrix failed: {}", e);
                    continue;
                }
            };
            let group_label = camera
                .which_camera
                .as_ref()
                .map(|id| id.group_name.as_str())
                .unwrap_or("");
            log::info!("New camera --> {}:{}", group_label, camera_name);
            let state = CameraState {
                info: camera,
                matrix,
            };
            cameras.write().insert(camera_name, state);

            // Emit a camera-model update immediately when cameras connect.
            let camera_models: Vec<CameraModel> = {
                let guard = cameras.read();
                guard
                    .values()
                    .filter_map(|cs| camera_model_from_info(&cs.info))
                    .collect()
            };
            let _ = stream_tx.send(PoseStreamUpdate {
                labeled_poses: LabeledPoses3D {
                    group_name: group_name.to_string(),
                    poses: vec![],
                    time: Instant::now(),
                },
                camera_views: vec![],
                cameras: camera_models,
            });
        }
    }

    async fn triangulate_loop(
        mut config: TriangulatorConfig,
        mut config_rx: Option<broadcast::Receiver<TriangulatorConfig>>,
        group_name: String,
        poses: Arc<RwLock<HashMap<String, Snapshot>>>,
        cameras: Arc<RwLock<HashMap<String, CameraState>>>,
        stream_tx: broadcast::Sender<PoseStreamUpdate>,
    ) -> Result<(), BoxError> {
        let mut smoothing_state = HashMap::new();

        loop {
            while let Some(ref mut rx) = config_rx {
                match rx.try_recv() {
                    Ok(c) => {
                        config = c;
                        log::debug!("Triangulator [{}]: config updated (reload)", group_name);
                    }
                    Err(broadcast::error::TryRecvError::Lagged(n)) => {
                        log::debug!("Triangulator [{}]: config reload lagged, skipped {} update(s)", group_name, n);
                    }
                    _ => break,
                }
            }

            // Clone under read lock so CPU-heavy work can run in spawn_blocking without holding locks.
            let (poses_clone, cameras_clone) = {
                let p = poses.read();
                let c = cameras.read();
                (p.clone(), c.clone())
            };

            let config_clone = config.clone();
            let group_name_clone = group_name.clone();
            let smoothing_state_for_blocking = smoothing_state.clone();
            let (update, new_smoothing_state) = match tokio::task::spawn_blocking(move || {
                triangulate_tick_blocking(
                    config_clone,
                    group_name_clone,
                    poses_clone,
                    cameras_clone,
                    smoothing_state_for_blocking,
                )
            })
            .await
            {
                Ok(result) => result,
                Err(e) => {
                    log::error!("Triangulator [{}]: spawn_blocking join error: {}", group_name, e);
                    sleep(config.poll_interval).await;
                    continue;
                }
            };

            smoothing_state = new_smoothing_state;
            if let Some(u) = update {
                let _ = stream_tx.send(u);
            }

            sleep(config.poll_interval).await;
        }
    }

    #[allow(dead_code)]
    fn get_pose_for_user(&self, pose: &Snapshot, user_id: usize) -> Option<Pose2D> {
        get_pose_for_user_impl(pose, user_id)
    }

    #[allow(dead_code)]
    fn group_poses_and_cameras_by_user(
        &self,
        snapshots: Vec<Snapshot>,
    ) -> Result<Vec<(Vec<Pose2D>, Vec<Matrix3x4<f64>>)>, HubError> {
        let cameras_guard = self.cameras.read();
        group_poses_and_cameras_by_user_impl(&snapshots, &cameras_guard, 1)
    }

    #[allow(dead_code)]
    fn get_camera_matrix(&self, name: &str) -> Option<Matrix3x4<f64>> {
        let hm = self.cameras.read();
        let camera = hm.get(name)?;
        Some(camera.matrix)
    }

    #[allow(dead_code)]
    fn get_cameras_for_snapshots(
        &self,
        snapshots: &[Snapshot],
    ) -> Result<Vec<Matrix3x4<f64>>, HubError> {
        get_cameras_for_snapshots_impl(snapshots, &*self.cameras.read())
    }
}

/// Filter current snapshots by pose expiration. Snapshots without a valid timestamp are skipped.
/// Uses the most recent snapshot timestamp as the reference (not wall-clock now) so that cameras
/// that sent slightly earlier are not dropped when another camera sends later—this keeps
/// multiple cameras in the same update for the frontend.
fn get_current_snapshot_impl(
    config: &TriangulatorConfig,
    poses: &HashMap<String, Snapshot>,
    now: SystemTime,
) -> Vec<Snapshot> {
    let with_ts: Vec<(SystemTime, Snapshot)> = poses
        .values()
        .filter_map(|snapshot| {
            let timestamp = snapshot.timestamp.as_ref()?;
            let snapshot_time =
                std::convert::TryInto::<SystemTime>::try_into(timestamp.clone()).ok()?;
            Some((snapshot_time, snapshot.clone()))
        })
        .collect();
    if with_ts.is_empty() {
        return vec![];
    }
    let ref_time = with_ts
        .iter()
        .map(|(t, _)| *t)
        .max()
        .unwrap_or(now);
    with_ts
        .into_iter()
        .filter_map(|(snapshot_time, snapshot)| {
            let age = ref_time.duration_since(snapshot_time).ok()?;
            if age < config.pose_expiration {
                Some(snapshot)
            } else {
                None
            }
        })
        .collect()
}

/// Get pose for a user index. Returns None if user_id is out of bounds (panic-safe).
pub fn get_pose_for_user_impl(pose: &Snapshot, user_id: usize) -> Option<Pose2D> {
    pose.poses.get(user_id).cloned()
}

/// Get camera matrices for snapshots in order. Requires which_camera and matching camera state.
pub fn get_cameras_for_snapshots_impl(
    snapshots: &[Snapshot],
    cameras: &HashMap<String, CameraState>,
) -> Result<Vec<Matrix3x4<f64>>, HubError> {
    snapshots
        .iter()
        .map(|snapshot| {
            let camera_name = snapshot
                .which_camera
                .as_ref()
                .map(|w| w.camera_name.as_str())
                .ok_or(MissingField::CameraName)?;
            cameras
                .get(camera_name)
                .map(|c| c.matrix)
                .ok_or(CalculationError::CameraMatrixFailed(MissingField::CameraName).into())
        })
        .collect()
}

/// Group snapshots by user: for each user index, collect Pose2D per snapshot and camera matrices in same order.
pub fn group_poses_and_cameras_by_user_impl(
    snapshots: &[Snapshot],
    cameras: &HashMap<String, CameraState>,
    max_users: usize,
) -> Result<Vec<(Vec<Pose2D>, Vec<Matrix3x4<f64>>)>, HubError> {
    let camera_matrices = get_cameras_for_snapshots_impl(snapshots, cameras)?;
    (0..max_users)
        .map(|user_id| {
            let user_poses: Vec<Pose2D> = snapshots
                .iter()
                .map(|s| {
                    get_pose_for_user_impl(s, user_id)
                        .ok_or(MissingField::Keypoint("user pose".to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok((user_poses, camera_matrices.clone()))
        })
        .collect()
}

/// One triangulator tick: CPU-heavy work intended to run in `tokio::task::spawn_blocking`.
/// Takes cloned poses/cameras and smoothing state; returns an optional stream update and updated smoothing state.
pub fn triangulate_tick_blocking(
    config: TriangulatorConfig,
    group_name: String,
    poses: HashMap<String, Snapshot>,
    cameras: HashMap<String, CameraState>,
    mut smoothing_state: HashMap<String, Camera2DTracks>,
) -> (Option<PoseStreamUpdate>, HashMap<String, Camera2DTracks>) {
    let now = SystemTime::now();
    let current = get_current_snapshot_impl(&config, &poses, now);
    if current.is_empty() {
        return (None, smoothing_state);
    }

    let current_smoothed = match &config.smoothing {
        Some(smoothing_config) => smooth_snapshots(&current, &mut smoothing_state, smoothing_config),
        None => current.clone(),
    };

    let camera_views: Vec<CameraView> = current_smoothed
        .iter()
        .filter_map(|s| {
            let name = s.which_camera.as_ref()?.camera_name.clone();
            Some(CameraView {
                camera_name: name,
                poses: s.poses.clone(),
            })
        })
        .collect();

    let match_threshold_px = config
        .pose_matching_threshold_px
        .unwrap_or(POSE_MATCHING_REPROJECTION_THRESHOLD_PX);

    let (camera_matrices, groups) = match get_cameras_for_snapshots_impl(&current_smoothed, &cameras) {
        Ok(matrices) => {
            let groups = match_poses(&current_smoothed, &matrices, match_threshold_px);
            (matrices, groups)
        }
        Err(e) => {
            log::warn!(
                "Triangulator [{}]: get_cameras_for_snapshots failed: {}",
                group_name,
                e
            );
            (vec![], vec![])
        }
    };

    if camera_matrices.is_empty() {
        return (None, smoothing_state);
    }

    let triangulated: Vec<Pose3D> = groups
        .par_iter()
        .filter_map(|group: &MatchedGroup| {
            let (poses_3d, matrices) = group.to_poses_and_matrices(&camera_matrices)?;
            let n_views = matrices.len();
            if n_views < config.min_cameras {
                return None;
            }
            match triangulate_from_poses_and_camera_matrices_partial(&poses_3d, &matrices) {
                Ok(pose3d) => {
                    log::info!(
                        "Triangulator [{}]: triangulated 1 pose ({} views), broadcasting",
                        group_name,
                        n_views
                    );
                    Some(pose3d)
                }
                Err(e) => {
                    log::warn!(
                        "Triangulator [{}]: triangulate failed ({} views): {}",
                        group_name,
                        n_views,
                        e
                    );
                    None
                }
            }
        })
        .collect();

    let labeled_poses = LabeledPoses3D {
        group_name,
        poses: triangulated,
        time: Instant::now(),
    };

    let update = PoseStreamUpdate {
        labeled_poses,
        camera_views,
        cameras: vec![],
    };
    (Some(update), smoothing_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::{
        CameraExtrinsics, CameraIdentifier, CameraInfo, CameraIntrinsics, Point2D, Snapshot,
    };
    use crate::openmvg::openmvg::{create_camera_matrix, get_projection};
    use nalgebra::{Point3, Rotation3};
    use prost_types::Timestamp;
    use rand::Rng;

    fn make_camera_identifier(group: &str, camera: &str) -> CameraIdentifier {
        CameraIdentifier {
            group_name: group.to_string(),
            camera_name: camera.to_string(),
        }
    }

    fn make_calibration_identity_like() -> crate::grpc::proto::CalibrationParameters {
        // 3x3 identity-like intrinsics, 3x4 view matrix (first 3x3 R, then column t)
        let camera_matrix = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let view_matrix = vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        crate::grpc::proto::CalibrationParameters {
            intrinsics: Some(CameraIntrinsics {
                camera_matrix,
                distortion: vec![],
                rms_error: 0.0,
            }),
            extrinsics: Some(CameraExtrinsics { view_matrix }),
        }
    }

    #[test]
    fn test_calculate_camera_matrix_valid() {
        let cal = make_calibration_identity_like();
        let p = calculate_camera_matrix(&cal).unwrap();
        assert_eq!(p.nrows(), 3);
        assert_eq!(p.ncols(), 4);
    }

    #[test]
    fn test_calculate_camera_matrix_missing_intrinsics() {
        let cal = crate::grpc::proto::CalibrationParameters {
            intrinsics: None,
            extrinsics: Some(CameraExtrinsics {
                view_matrix: vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            }),
        };
        assert!(calculate_camera_matrix(&cal).is_err());
    }

    #[test]
    fn test_triangulate_from_poses_and_camera_matrices_roundtrip() {
        // Use fixed, well-conditioned geometry: two cameras with good baseline and a point in front of both.
        // Random camera setups often produce degenerate geometry (small baseline, point behind camera).
        let c1 = Point3::new(0.0, 0.0, 0.0);
        let r1 = Rotation3::identity();
        let c2 = Point3::new(2.0, 0.0, 0.0);
        let r2 = Rotation3::identity();
        let p1 = create_camera_matrix(c1, r1);
        let p2 = create_camera_matrix(c2, r2);
        let cameras = vec![p1.clone(), p2.clone()];

        let x3d = Point3::new(1.0, 0.0, 5.0);
        let pose2d_1 = get_projection(x3d, p1).unwrap();
        let pose2d_2 = get_projection(x3d, p2).unwrap();

        let point2d_1 = Point2D {
            x: pose2d_1.x,
            y: pose2d_1.y,
            score: 1.0,
        };
        let point2d_2 = Point2D {
            x: pose2d_2.x,
            y: pose2d_2.y,
            score: 1.0,
        };
        let pt1 = point2d_1.clone();
        let pose_2d_1 = crate::grpc::proto::Pose2D {
            nose: Some(point2d_1),
            left_eye: Some(pt1.clone()),
            right_eye: Some(pt1.clone()),
            left_ear: Some(pt1.clone()),
            right_ear: Some(pt1.clone()),
            left_shoulder: Some(pt1.clone()),
            right_shoulder: Some(pt1.clone()),
            left_elbow: Some(pt1.clone()),
            right_elbow: Some(pt1.clone()),
            left_wrist: Some(pt1.clone()),
            right_wrist: Some(pt1.clone()),
            left_hip: Some(pt1.clone()),
            right_hip: Some(pt1.clone()),
            left_knee: Some(pt1.clone()),
            right_knee: Some(pt1.clone()),
            left_ankle: Some(pt1.clone()),
            right_ankle: Some(pt1),
            score: 1.0,
        };
        let pt2 = point2d_2.clone();
        let pose_2d_2 = crate::grpc::proto::Pose2D {
            nose: Some(point2d_2),
            left_eye: Some(pt2.clone()),
            right_eye: Some(pt2.clone()),
            left_ear: Some(pt2.clone()),
            right_ear: Some(pt2.clone()),
            left_shoulder: Some(pt2.clone()),
            right_shoulder: Some(pt2.clone()),
            left_elbow: Some(pt2.clone()),
            right_elbow: Some(pt2.clone()),
            left_wrist: Some(pt2.clone()),
            right_wrist: Some(pt2.clone()),
            left_hip: Some(pt2.clone()),
            right_hip: Some(pt2.clone()),
            left_knee: Some(pt2.clone()),
            right_knee: Some(pt2.clone()),
            left_ankle: Some(pt2.clone()),
            right_ankle: Some(pt2),
            score: 1.0,
        };
        let poses = vec![pose_2d_1, pose_2d_2];

        let pose3d = triangulate_from_poses_and_camera_matrices(poses, &cameras).unwrap();
        let pts = pose3d.nose.as_ref().unwrap();
        let rec = Point3::new(pts.x, pts.y, pts.z);
        let err = (rec - x3d).norm();
        assert!(err < 1e-5, "reconstruction error {} for x3d {:?}", err, x3d);
    }

    /// Integration: smooth_snapshots → match_poses → triangulate yields valid 3D pose.
    #[test]
    fn test_smooth_then_match_then_triangulate() {
        use crate::grpc::proto::CalibrationParameters;
        use crate::triangulator::pose_smoothing::SmoothingConfig;

        let c1 = Point3::new(0.0, 0.0, 0.0);
        let r1 = Rotation3::identity();
        let c2 = Point3::new(2.0, 0.0, 0.0);
        let r2 = Rotation3::identity();
        let p1 = create_camera_matrix(c1, r1);
        let p2 = create_camera_matrix(c2, r2);

        let x3d = Point3::new(1.0, 0.0, 5.0);
        let pose2d_1 = get_projection(x3d, p1.clone()).unwrap();
        let pose2d_2 = get_projection(x3d, p2.clone()).unwrap();

        let point2d_1 = Point2D {
            x: pose2d_1.x,
            y: pose2d_1.y,
            score: 1.0,
        };
        let point2d_2 = Point2D {
            x: pose2d_2.x,
            y: pose2d_2.y,
            score: 1.0,
        };
        let pt1 = point2d_1.clone();
        let pose_2d_1 = crate::grpc::proto::Pose2D {
            nose: Some(point2d_1),
            left_eye: Some(pt1.clone()),
            right_eye: Some(pt1.clone()),
            left_ear: Some(pt1.clone()),
            right_ear: Some(pt1.clone()),
            left_shoulder: Some(pt1.clone()),
            right_shoulder: Some(pt1.clone()),
            left_elbow: Some(pt1.clone()),
            right_elbow: Some(pt1.clone()),
            left_wrist: Some(pt1.clone()),
            right_wrist: Some(pt1.clone()),
            left_hip: Some(pt1.clone()),
            right_hip: Some(pt1.clone()),
            left_knee: Some(pt1.clone()),
            right_knee: Some(pt1.clone()),
            left_ankle: Some(pt1.clone()),
            right_ankle: Some(pt1),
            score: 1.0,
        };
        let pt2 = point2d_2.clone();
        let pose_2d_2 = crate::grpc::proto::Pose2D {
            nose: Some(point2d_2),
            left_eye: Some(pt2.clone()),
            right_eye: Some(pt2.clone()),
            left_ear: Some(pt2.clone()),
            right_ear: Some(pt2.clone()),
            left_shoulder: Some(pt2.clone()),
            right_shoulder: Some(pt2.clone()),
            left_elbow: Some(pt2.clone()),
            right_elbow: Some(pt2.clone()),
            left_wrist: Some(pt2.clone()),
            right_wrist: Some(pt2.clone()),
            left_hip: Some(pt2.clone()),
            right_hip: Some(pt2.clone()),
            left_knee: Some(pt2.clone()),
            right_knee: Some(pt2.clone()),
            left_ankle: Some(pt2.clone()),
            right_ankle: Some(pt2),
            score: 1.0,
        };

        fn matrix_to_view_matrix(p: &nalgebra::Matrix3x4<f64>) -> Vec<f64> {
            (0..4).flat_map(|c| (0..3).map(move |r| p[(r, c)])).collect()
        }

        let cal1 = CalibrationParameters {
            intrinsics: Some(CameraIntrinsics {
                camera_matrix: vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                distortion: vec![],
                rms_error: 0.0,
            }),
            extrinsics: Some(CameraExtrinsics {
                view_matrix: matrix_to_view_matrix(&p1),
            }),
        };
        let cal2 = CalibrationParameters {
            intrinsics: Some(CameraIntrinsics {
                camera_matrix: vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                distortion: vec![],
                rms_error: 0.0,
            }),
            extrinsics: Some(CameraExtrinsics {
                view_matrix: matrix_to_view_matrix(&p2),
            }),
        };

        let snap0 = Snapshot {
            timestamp: None,
            which_camera: Some(make_camera_identifier("g", "cam0")),
            poses: vec![pose_2d_1],
            image: None,
        };
        let snap1 = Snapshot {
            timestamp: None,
            which_camera: Some(make_camera_identifier("g", "cam1")),
            poses: vec![pose_2d_2],
            image: None,
        };
        let current = vec![snap0, snap1];

        let mut smoothing_state = HashMap::new();
        let config = SmoothingConfig::default();
        let smoothed = smooth_snapshots(&current, &mut smoothing_state, &config);
        assert_eq!(smoothed.len(), 2);
        assert_eq!(smoothed[0].poses.len(), 1);
        assert_eq!(smoothed[1].poses.len(), 1);

        let mut cameras = HashMap::new();
        cameras.insert(
            "cam0".to_string(),
            CameraState {
                info: CameraInfo {
                    which_camera: Some(make_camera_identifier("g", "cam0")),
                    calibration: Some(cal1),
                },
                matrix: p1,
            },
        );
        cameras.insert(
            "cam1".to_string(),
            CameraState {
                info: CameraInfo {
                    which_camera: Some(make_camera_identifier("g", "cam1")),
                    calibration: Some(cal2),
                },
                matrix: p2,
            },
        );

        let matrices = get_cameras_for_snapshots_impl(&smoothed, &cameras).unwrap();
        assert_eq!(matrices.len(), 2);

        let groups = match_poses(&smoothed, &matrices, POSE_MATCHING_REPROJECTION_THRESHOLD_PX);
        assert_eq!(groups.len(), 1);

        let results = triangulate_matched_groups(&groups, &matrices);
        assert_eq!(results.len(), 1);
        let pose3d = results[0].as_ref().unwrap();
        let pts = pose3d.nose.as_ref().unwrap();
        let rec = Point3::new(pts.x, pts.y, pts.z);
        let err = (rec - x3d).norm();
        assert!(err < 0.1, "reconstruction error {} for x3d {:?} (smoothed path)", err, x3d);
    }

    #[test]
    fn test_triangulate_partial_pose_missing_keypoint_yields_none() {
        // Same geometry as roundtrip: two cameras, one 3D point. Camera 0 has nose missing;
        // camera 1 has all keypoints. Nose has only 1 observation → nose 3D is None; others have 2 → Some.
        let c1 = Point3::new(0.0, 0.0, 0.0);
        let r1 = Rotation3::identity();
        let c2 = Point3::new(2.0, 0.0, 0.0);
        let r2 = Rotation3::identity();
        let p1 = create_camera_matrix(c1, r1);
        let p2 = create_camera_matrix(c2, r2);
        let cameras = vec![p1.clone(), p2.clone()];

        let x3d = Point3::new(1.0, 0.0, 5.0);
        let pose2d_1 = get_projection(x3d, p1.clone()).unwrap();
        let pose2d_2 = get_projection(x3d, p2).unwrap();

        let point2d_1 = Point2D {
            x: pose2d_1.x,
            y: pose2d_1.y,
            score: 1.0,
        };
        let point2d_2 = Point2D {
            x: pose2d_2.x,
            y: pose2d_2.y,
            score: 1.0,
        };

        let pt1 = point2d_1.clone();
        let pose_2d_1 = crate::grpc::proto::Pose2D {
            nose: None, // missing in camera 0
            left_eye: Some(pt1.clone()),
            right_eye: Some(pt1.clone()),
            left_ear: Some(pt1.clone()),
            right_ear: Some(pt1.clone()),
            left_shoulder: Some(pt1.clone()),
            right_shoulder: Some(pt1.clone()),
            left_elbow: Some(pt1.clone()),
            right_elbow: Some(pt1.clone()),
            left_wrist: Some(pt1.clone()),
            right_wrist: Some(pt1.clone()),
            left_hip: Some(pt1.clone()),
            right_hip: Some(pt1.clone()),
            left_knee: Some(pt1.clone()),
            right_knee: Some(pt1.clone()),
            left_ankle: Some(pt1.clone()),
            right_ankle: Some(pt1),
            score: 1.0,
        };
        let pt2 = point2d_2.clone();
        let pose_2d_2 = crate::grpc::proto::Pose2D {
            nose: Some(point2d_2.clone()),
            left_eye: Some(pt2.clone()),
            right_eye: Some(pt2.clone()),
            left_ear: Some(pt2.clone()),
            right_ear: Some(pt2.clone()),
            left_shoulder: Some(pt2.clone()),
            right_shoulder: Some(pt2.clone()),
            left_elbow: Some(pt2.clone()),
            right_elbow: Some(pt2.clone()),
            left_wrist: Some(pt2.clone()),
            right_wrist: Some(pt2.clone()),
            left_hip: Some(pt2.clone()),
            right_hip: Some(pt2.clone()),
            left_knee: Some(pt2.clone()),
            right_knee: Some(pt2.clone()),
            left_ankle: Some(pt2.clone()),
            right_ankle: Some(pt2),
            score: 1.0,
        };
        let poses = vec![pose_2d_1, pose_2d_2];

        let pose3d =
            triangulate_from_poses_and_camera_matrices_partial(&poses, &cameras).unwrap();
        assert!(pose3d.nose.is_none(), "nose had only 1 observation, should be None");
        assert!(
            pose3d.left_eye.is_some(),
            "left_eye had 2 observations, should be Some"
        );
        let rec = Point3::new(
            pose3d.left_eye.as_ref().unwrap().x,
            pose3d.left_eye.as_ref().unwrap().y,
            pose3d.left_eye.as_ref().unwrap().z,
        );
        let err = (rec - x3d).norm();
        assert!(err < 1e-5, "reconstruction error {} for left_eye", err);
    }

    #[test]
    fn test_get_pose_for_user_empty_poses_returns_none() {
        let snapshot = Snapshot {
            timestamp: None,
            which_camera: None,
            poses: vec![],
            image: None,
        };
        assert!(get_pose_for_user_impl(&snapshot, 0).is_none());
    }

    #[test]
    fn test_get_pose_for_user_one_pose_returns_some() {
        let pose = rand::random::<Pose2D>();
        let snapshot = Snapshot {
            timestamp: None,
            which_camera: None,
            poses: vec![pose.clone()],
            image: None,
        };
        let got = get_pose_for_user_impl(&snapshot, 0).unwrap();
        assert_eq!(got.score, pose.score);
    }

    #[test]
    fn test_get_pose_for_user_user_id_out_of_bounds_returns_none() {
        let pose = rand::random::<Pose2D>();
        let snapshot = Snapshot {
            timestamp: None,
            which_camera: None,
            poses: vec![pose],
            image: None,
        };
        assert!(get_pose_for_user_impl(&snapshot, 1).is_none());
    }

    fn timestamp_from_system_time(t: SystemTime) -> Option<Timestamp> {
        t.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| Timestamp {
                seconds: d.as_secs() as i64,
                nanos: d.subsec_nanos() as i32,
            })
    }

    #[test]
    fn test_get_current_snapshot_filters_expired() {
        let config = TriangulatorConfig {
            pose_expiration: Duration::from_millis(100),
            poll_interval: Duration::from_millis(16),
            min_cameras: 2,
            pose_matching_threshold_px: None,
            smoothing: None,
        };
        let mut poses = HashMap::new();
        let now = SystemTime::now();
        let fresh_ts = timestamp_from_system_time(now).unwrap();
        let old_ts = timestamp_from_system_time(now - Duration::from_secs(10)).unwrap();

        poses.insert(
            "cam_fresh".to_string(),
            Snapshot {
                timestamp: Some(fresh_ts),
                which_camera: Some(make_camera_identifier("g", "cam_fresh")),
                poses: vec![],
                image: None,
            },
        );
        poses.insert(
            "cam_old".to_string(),
            Snapshot {
                timestamp: Some(old_ts),
                which_camera: Some(make_camera_identifier("g", "cam_old")),
                poses: vec![],
                image: None,
            },
        );
        let current = get_current_snapshot_impl(&config, &poses, now);
        assert_eq!(current.len(), 1);
        assert_eq!(
            current[0].which_camera.as_ref().unwrap().camera_name,
            "cam_fresh"
        );
    }

    #[test]
    fn test_get_current_snapshot_skips_missing_timestamp() {
        let config = TriangulatorConfig::default();
        let mut poses = HashMap::new();
        poses.insert(
            "cam_no_ts".to_string(),
            Snapshot {
                timestamp: None,
                which_camera: Some(make_camera_identifier("g", "cam_no_ts")),
                poses: vec![],
                image: None,
            },
        );
        let current = get_current_snapshot_impl(&config, &poses, SystemTime::now());
        assert!(current.is_empty());
    }

    #[test]
    fn test_get_cameras_for_snapshots_impl() {
        let cal = make_calibration_identity_like();
        let matrix = calculate_camera_matrix(&cal).unwrap();
        let mut cameras = HashMap::new();
        cameras.insert(
            "cam_a".to_string(),
            CameraState {
                info: CameraInfo {
                    which_camera: Some(make_camera_identifier("g", "cam_a")),
                    calibration: Some(cal),
                },
                matrix,
            },
        );
        let snapshots = vec![Snapshot {
            timestamp: None,
            which_camera: Some(make_camera_identifier("g", "cam_a")),
            poses: vec![],
            image: None,
        }];
        let matrices = get_cameras_for_snapshots_impl(&snapshots, &cameras).unwrap();
        assert_eq!(matrices.len(), 1);
        assert_eq!(matrices[0].nrows(), 3);
        assert_eq!(matrices[0].ncols(), 4);
    }

    #[test]
    fn test_get_cameras_for_snapshots_impl_missing_camera_errors() {
        let cameras = HashMap::new();
        let snapshots = vec![Snapshot {
            timestamp: None,
            which_camera: Some(make_camera_identifier("g", "unknown")),
            poses: vec![],
            image: None,
        }];
        assert!(get_cameras_for_snapshots_impl(&snapshots, &cameras).is_err());
    }

    #[test]
    fn test_group_poses_and_cameras_by_user_impl() {
        let cal = make_calibration_identity_like();
        let matrix = calculate_camera_matrix(&cal).unwrap();
        let mut cameras = HashMap::new();
        cameras.insert(
            "cam_a".to_string(),
            CameraState {
                info: CameraInfo {
                    which_camera: Some(make_camera_identifier("g", "cam_a")),
                    calibration: Some(cal.clone()),
                },
                matrix,
            },
        );
        let pose = rand::random::<Pose2D>();
        let snapshots = vec![Snapshot {
            timestamp: None,
            which_camera: Some(make_camera_identifier("g", "cam_a")),
            poses: vec![pose.clone()],
            image: None,
        }];
        let grouped = group_poses_and_cameras_by_user_impl(&snapshots, &cameras, 1).unwrap();
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].0.len(), 1);
        assert_eq!(grouped[0].1.len(), 1);
    }

    #[test]
    fn test_group_poses_and_cameras_by_user_impl_two_poses() {
        let cal = make_calibration_identity_like();
        let matrix = calculate_camera_matrix(&cal).unwrap();
        let mut cameras = HashMap::new();
        cameras.insert(
            "cam_a".to_string(),
            CameraState {
                info: CameraInfo {
                    which_camera: Some(make_camera_identifier("g", "cam_a")),
                    calibration: Some(cal.clone()),
                },
                matrix,
            },
        );
        let pose1 = rand::random::<Pose2D>();
        let pose2 = rand::random::<Pose2D>();
        let snapshots = vec![Snapshot {
            timestamp: None,
            which_camera: Some(make_camera_identifier("g", "cam_a")),
            poses: vec![pose1.clone(), pose2.clone()],
            image: None,
        }];
        let grouped = group_poses_and_cameras_by_user_impl(&snapshots, &cameras, 2).unwrap();
        assert_eq!(grouped.len(), 2, "should have one group per subject");
        assert_eq!(grouped[0].0.len(), 1);
        assert_eq!(grouped[0].1.len(), 1);
        assert_eq!(grouped[1].0.len(), 1);
        assert_eq!(grouped[1].1.len(), 1);
    }
}
