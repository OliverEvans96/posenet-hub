use nalgebra::{Matrix3, Matrix3x4, Point2};
use parking_lot::RwLock;
use std::convert::TryInto;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::{sync::broadcast, time::sleep, try_join};

use crate::errors::{CalculationError, HubError, MissingField};
use crate::grpc::proto::{CalibrationParameters, CameraInfo, Pose2D, Pose3D, SPoint2, Snapshot};
use crate::openmvg::openmvg::triangulate_many;
use crate::utils::transpose_vecvec;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

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

// pub fn score_from_poses(poses: Vec<Pose2D>, pose3d: Pose3D)

pub struct Triangulator {
    config: TriangulatorConfig,
    group_name: String,
    cameras_rx: UnboundedReceiver<CameraInfo>,
    snapshots_rx: UnboundedReceiver<Snapshot>,
    poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    cameras: Arc<RwLock<HashMap<String, CameraState>>>,
    poses: Arc<RwLock<HashMap<String, Snapshot>>>,
}

impl Triangulator {
    pub fn new(
        config: TriangulatorConfig,
        group_name: String,
        cameras_rx: UnboundedReceiver<CameraInfo>,
        snapshots_rx: UnboundedReceiver<Snapshot>,
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

    /// Run the triangulator: spawn listen_for_poses, listen_for_cameras, and triangulate_loop; join until one errors.
    pub async fn run(self) -> Result<(), BoxError> {
        let config = self.config.clone();
        let group_name = self.group_name.clone();
        let poses3d_tx = self.poses3d_tx;
        let cameras = self.cameras.clone();
        let poses = self.poses.clone();
        let snapshots_rx = self.snapshots_rx;
        let cameras_rx = self.cameras_rx;

        let poses_for_tri = poses.clone();
        let cameras_for_tri = cameras.clone();

        let poses_handle = tokio::spawn(async move {
            Self::listen_for_poses(snapshots_rx, poses).await
        });
        let cameras_handle = tokio::spawn(async move {
            Self::listen_for_cameras(cameras_rx, cameras).await
        });
        let triangulate_handle = tokio::spawn(async move {
            Self::triangulate_loop(config, group_name, poses_for_tri, cameras_for_tri, poses3d_tx)
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
            poses.write().insert(camera_name, snapshot);
        }
    }

    async fn listen_for_cameras(
        mut cameras_rx: UnboundedReceiver<CameraInfo>,
        cameras: Arc<RwLock<HashMap<String, CameraState>>>,
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
            let group_name = camera
                .which_camera
                .as_ref()
                .map(|id| id.group_name.as_str())
                .unwrap_or("");
            log::info!("New camera --> {}:{}", group_name, camera_name);
            let state = CameraState { info: camera, matrix };
            cameras.write().insert(camera_name, state);
        }
    }

    async fn triangulate_loop(
        config: TriangulatorConfig,
        group_name: String,
        poses: Arc<RwLock<HashMap<String, Snapshot>>>,
        cameras: Arc<RwLock<HashMap<String, CameraState>>>,
        poses3d_tx: broadcast::Sender<LabeledPoses3D>,
    ) -> Result<(), BoxError> {
        loop {
            let current =
                get_current_snapshot_impl(&config, &*poses.read(), SystemTime::now());
            let users = {
                let cameras_guard = cameras.read();
                group_poses_and_cameras_by_user_impl(&current, &cameras_guard, 1)
            };
            let users = match users {
                Ok(u) => u,
                Err(e) => {
                    log::warn!("group_poses_and_cameras_by_user failed: {}", e);
                    sleep(config.poll_interval).await;
                    continue;
                }
            };
            for (poses_2d, camera_matrices) in users {
                if camera_matrices.len() >= config.min_cameras {
                    match triangulate_from_poses_and_camera_matrices(poses_2d, &camera_matrices) {
                        Ok(pose3d) => {
                            let _ = poses3d_tx.send(LabeledPoses3D {
                                group_name: group_name.clone(),
                                poses: vec![pose3d],
                                time: Instant::now(),
                            });
                        }
                        Err(e) => {
                            log::warn!("triangulate failed: {}", e);
                        }
                    }
                } else {
                    let _ = poses3d_tx.send(LabeledPoses3D {
                        group_name: group_name.clone(),
                        poses: vec![],
                        time: Instant::now(),
                    });
                }
            }
            sleep(config.poll_interval).await;
        }
    }

    fn get_pose_for_user(&self, pose: &Snapshot, user_id: usize) -> Option<Pose2D> {
        get_pose_for_user_impl(pose, user_id)
    }

    fn group_poses_and_cameras_by_user(
        &self,
        snapshots: Vec<Snapshot>,
    ) -> Result<Vec<(Vec<Pose2D>, Vec<Matrix3x4<f64>>)>, HubError> {
        let cameras_guard = self.cameras.read();
        group_poses_and_cameras_by_user_impl(&snapshots, &cameras_guard, 1)
    }

    fn get_camera_matrix(&self, name: &str) -> Option<Matrix3x4<f64>> {
        let hm = self.cameras.read();
        let camera = hm.get(name)?;
        Some(camera.matrix)
    }

    fn get_cameras_for_snapshots(
        &self,
        snapshots: &[Snapshot],
    ) -> Result<Vec<Matrix3x4<f64>>, HubError> {
        get_cameras_for_snapshots_impl(snapshots, &*self.cameras.read())
    }
}

/// Filter current snapshots by pose expiration. Snapshots without a valid timestamp are skipped.
fn get_current_snapshot_impl(
    config: &TriangulatorConfig,
    poses: &HashMap<String, Snapshot>,
    now: SystemTime,
) -> Vec<Snapshot> {
    poses
        .values()
        .filter_map(|snapshot| {
            let timestamp = snapshot.timestamp.as_ref()?;
            let snapshot_time = std::convert::TryInto::<SystemTime>::try_into(timestamp.clone()).ok()?;
            let age = now.duration_since(snapshot_time).ok()?;
            if age < config.pose_expiration {
                Some(snapshot.clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::{
        CameraExtrinsics, CameraIdentifier, CameraIntrinsics, Point2D,
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
        let view_matrix = vec![
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ];
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
        let mut rng = rand::thread_rng();
        let c1 = Point3::new(rng.gen(), rng.gen(), rng.gen());
        let r1 = Rotation3::from_euler_angles(rng.gen(), rng.gen(), rng.gen());
        let c2 = Point3::new(rng.gen(), rng.gen(), rng.gen());
        let r2 = Rotation3::from_euler_angles(rng.gen(), rng.gen(), rng.gen());
        let p1 = create_camera_matrix(c1, r1);
        let p2 = create_camera_matrix(c2, r2);
        let cameras = vec![p1, p2];

        let x3d = Point3::new(rng.gen_range(0.1..10.0), rng.gen_range(0.1..10.0), rng.gen_range(0.1..10.0));
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
        let pt = point2d_1.clone();
        let pose_2d_1 = crate::grpc::proto::Pose2D {
            nose: Some(point2d_1),
            left_eye: Some(pt.clone()),
            right_eye: Some(pt.clone()),
            left_ear: Some(pt.clone()),
            right_ear: Some(pt.clone()),
            left_shoulder: Some(pt.clone()),
            right_shoulder: Some(pt.clone()),
            left_elbow: Some(pt.clone()),
            right_elbow: Some(pt.clone()),
            left_wrist: Some(pt.clone()),
            right_wrist: Some(pt.clone()),
            left_hip: Some(pt.clone()),
            right_hip: Some(pt.clone()),
            left_knee: Some(pt.clone()),
            right_knee: Some(pt.clone()),
            left_ankle: Some(pt.clone()),
            right_ankle: Some(pt),
            score: 1.0,
        };
        let pose_2d_2 = pose_2d_1.clone();
        let poses = vec![pose_2d_1, pose_2d_2];

        let pose3d = triangulate_from_poses_and_camera_matrices(poses, &cameras).unwrap();
        let pts = pose3d.nose.as_ref().unwrap();
        let rec = Point3::new(pts.x, pts.y, pts.z);
        let err = (rec - x3d).norm();
        assert!(err < 1e-5, "reconstruction error {} for x3d {:?}", err, x3d);
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
        let snapshots = vec![
            Snapshot {
                timestamp: None,
                which_camera: Some(make_camera_identifier("g", "cam_a")),
                poses: vec![pose.clone()],
                image: None,
            },
        ];
        let grouped = group_poses_and_cameras_by_user_impl(&snapshots, &cameras, 1).unwrap();
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].0.len(), 1);
        assert_eq!(grouped[0].1.len(), 1);
    }
}
