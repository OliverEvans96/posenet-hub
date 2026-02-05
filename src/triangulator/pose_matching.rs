//! Pose matching across cameras by 3D consistency (reprojection error).
//!
//! Matches 2D poses from multiple cameras so that each group corresponds to the same person.
//! Uses triangulation + reprojection as the sole cost; Hungarian assignment for two views,
//! greedy extension to the third; falls back to index-based grouping when matching fails.

use nalgebra::{Matrix3x4, Point2, Point3};
use std::f64;

use crate::errors::HubError;
use crate::grpc::proto::{Pose2D, Pose3D};
use crate::openmvg::openmvg::get_projection;
use super::triangulate_from_poses_and_camera_matrices_partial;

/// One matched group: one 2D pose per camera (None if that camera did not see this person).
/// Length = number of cameras; indices match the snapshot/camera order.
#[derive(Debug, Clone)]
pub struct MatchedGroup(pub Vec<Option<Pose2D>>);

impl MatchedGroup {
    /// Poses that are Some, in order (for triangulation).
    pub fn poses(&self) -> Vec<&Pose2D> {
        self.0.iter().filter_map(|p| p.as_ref()).collect()
    }

    /// Number of views (cameras) with a pose.
    pub fn view_count(&self) -> usize {
        self.0.iter().filter(|p| p.is_some()).count()
    }

    /// Convert to (poses, matrices) for triangulation: only cameras with a pose.
    pub fn to_poses_and_matrices(
        &self,
        camera_matrices: &[Matrix3x4<f64>],
    ) -> Option<(Vec<Pose2D>, Vec<Matrix3x4<f64>>)> {
        let poses: Vec<Pose2D> = self.0.iter().filter_map(|p| p.clone()).collect();
        let matrices: Vec<Matrix3x4<f64>> = self
            .0
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().map(|_| camera_matrices[i]))
            .collect();
        if poses.len() == matrices.len() && poses.len() >= 2 {
            Some((poses, matrices))
        } else {
            None
        }
    }
}

/// Keypoints from Pose3D as optional nalgebra points (order: nose..right_ankle).
fn pose3d_keypoints(pose: &Pose3D) -> [Option<Point3<f64>>; 17] {
    let pt = |p: &Option<crate::grpc::proto::Point3D>| {
        p.as_ref().map(|q| Point3::new(q.x, q.y, q.z))
    };
    [
        pt(&pose.nose),
        pt(&pose.left_eye),
        pt(&pose.right_eye),
        pt(&pose.left_ear),
        pt(&pose.right_ear),
        pt(&pose.left_shoulder),
        pt(&pose.right_shoulder),
        pt(&pose.left_elbow),
        pt(&pose.right_elbow),
        pt(&pose.left_wrist),
        pt(&pose.right_wrist),
        pt(&pose.left_hip),
        pt(&pose.right_hip),
        pt(&pose.left_knee),
        pt(&pose.right_knee),
        pt(&pose.left_ankle),
        pt(&pose.right_ankle),
    ]
}

/// Keypoints from Pose2D as optional nalgebra points (order: nose..right_ankle).
fn pose2d_keypoints(pose: &Pose2D) -> [Option<Point2<f64>>; 17] {
    let pt = |p: &Option<crate::grpc::proto::Point2D>| {
        p.as_ref().map(|q| Point2::new(q.x, q.y))
    };
    [
        pt(&pose.nose),
        pt(&pose.left_eye),
        pt(&pose.right_eye),
        pt(&pose.left_ear),
        pt(&pose.right_ear),
        pt(&pose.left_shoulder),
        pt(&pose.right_shoulder),
        pt(&pose.left_elbow),
        pt(&pose.right_elbow),
        pt(&pose.left_wrist),
        pt(&pose.right_wrist),
        pt(&pose.left_hip),
        pt(&pose.right_hip),
        pt(&pose.left_knee),
        pt(&pose.right_knee),
        pt(&pose.left_ankle),
        pt(&pose.right_ankle),
    ]
}

/// Sum of reprojection distances over keypoints present in both pose3d and pose2d.
/// Project pose3d into camera P, compare to pose2d. Returns f64::INFINITY if no keypoints overlap.
fn reprojection_error(pose3d: &Pose3D, pose2d: &Pose2D, p: Matrix3x4<f64>) -> f64 {
    let k3 = pose3d_keypoints(pose3d);
    let k2 = pose2d_keypoints(pose2d);
    let mut sum = 0.0_f64;
    let mut count = 0_usize;
    for (opt3, opt2) in k3.iter().zip(k2.iter()) {
        if let (Some(pt3), Some(pt2)) = (opt3, opt2) {
            if let Ok(proj) = get_projection(*pt3, p) {
                sum += (proj - *pt2).norm();
                count += 1;
            }
        }
    }
    if count == 0 {
        f64::INFINITY
    } else {
        sum / (count as f64)
    }
}

/// Cost for pairing pose_i (cam0) and pose_j (cam1): triangulate, then min reprojection error in cam2.
/// Returns (cost, best_cam2_index). Cost is f64::INFINITY if triangulation fails or no valid cam2 pose.
fn cost_pair_01(
    pose0: &Pose2D,
    pose1: &Pose2D,
    cam2_poses: &[Pose2D],
    p0: Matrix3x4<f64>,
    p1: Matrix3x4<f64>,
    p2: Matrix3x4<f64>,
) -> (f64, Option<usize>) {
    let pose3d = match triangulate_from_poses_and_camera_matrices_partial(
        &[pose0.clone(), pose1.clone()],
        &[p0, p1],
    ) {
        Ok(p) => p,
        Err(_) => return (f64::INFINITY, None),
    };
    let mut best_cost = f64::INFINITY;
    let mut best_k = None;
    for (k, pose2) in cam2_poses.iter().enumerate() {
        let err = reprojection_error(&pose3d, pose2, p2);
        if err < best_cost {
            best_cost = err;
            best_k = Some(k);
        }
    }
    (best_cost, best_k)
}

/// Hungarian (Kuhn–Munkres) min-cost assignment for a square cost matrix.
/// cost[i][j] = cost of assigning row i to column j. Returns vec of (row, col) assignments.
/// For rectangular matrices, pad with f64::INFINITY to make square.
fn hungarian_min_cost(cost: &[Vec<f64>]) -> Vec<(usize, usize)> {
    let n = cost.len();
    if n == 0 {
        return vec![];
    }
    let m = cost[0].len();
    let size = n.max(m);
    if size == 0 {
        return vec![];
    }
    let mut c = vec![vec![f64::INFINITY; size]; size];
    for (i, row) in cost.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            c[i][j] = v;
        }
    }
    let mut u = vec![0.0; size + 1];
    let mut v = vec![0.0; size + 1];
    let mut p = vec![0; size + 1];
    let mut way = vec![0; size + 1];
    for i in 1..=size {
        p[0] = i;
        let mut j0 = 0;
        let mut minv = vec![f64::INFINITY; size + 1];
        let mut used = vec![false; size + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0;
            for j in 1..=size {
                if used[j] {
                    continue;
                }
                let cur = c[i0 - 1][j - 1] - u[i0] - v[j];
                if cur < minv[j] {
                    minv[j] = cur;
                    way[j] = j0;
                }
                if minv[j] < delta {
                    delta = minv[j];
                    j1 = j;
                }
            }
            for j in 0..=size {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut out = Vec::new();
    for j in 1..=size {
        let i = p[j];
        if i != 0 && i <= n && j <= m && c[i - 1][j - 1] < f64::INFINITY {
            out.push((i - 1, j - 1));
        }
    }
    out
}

/// Match poses across 3 cameras: Hungarian(cam0, cam1) then greedy cam2.
/// Snapshots and camera_matrices must be in same order, length 3.
/// Pairs with cost > threshold are rejected. Returns matched groups; falls back to index-based if empty.
pub fn match_poses_3cam(
    snapshots: &[crate::grpc::proto::Snapshot],
    camera_matrices: &[Matrix3x4<f64>],
    threshold: f64,
) -> Vec<MatchedGroup> {
    if snapshots.len() != 3 || camera_matrices.len() != 3 {
        return vec![];
    }
    let s0 = &snapshots[0].poses;
    let s1 = &snapshots[1].poses;
    let s2 = &snapshots[2].poses;
    let p0 = camera_matrices[0];
    let p1 = camera_matrices[1];
    let p2 = camera_matrices[2];
    let n0 = s0.len();
    let n1 = s1.len();
    if n0 == 0 || n1 == 0 {
        return vec![];
    }
    let mut cost = vec![vec![f64::INFINITY; n1]; n0];
    let mut best_k2 = vec![vec![None; n1]; n0];
    for i in 0..n0 {
        for j in 0..n1 {
            let (c, k2) = cost_pair_01(&s0[i], &s1[j], s2, p0, p1, p2);
            cost[i][j] = c;
            best_k2[i][j] = k2;
        }
    }
    let assignments = hungarian_min_cost(&cost);
    let mut used_cam2 = std::collections::HashSet::new();
    let mut groups = Vec::new();
    let mut by_cost: Vec<_> = assignments
        .into_iter()
        .map(|(i, j)| {
            let c = cost[i][j];
            (c, i, j)
        })
        .collect();
    by_cost.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (c, i, j) in by_cost {
        if c > threshold {
            continue;
        }
        let k2 = best_k2[i][j];
        let k2 = match k2 {
            Some(k) if !used_cam2.contains(&k) => k,
            _ => continue,
        };
        used_cam2.insert(k2);
        let group = MatchedGroup(vec![
            Some(s0[i].clone()),
            Some(s1[j].clone()),
            Some(s2[k2].clone()),
        ]);
        groups.push(group);
    }
    groups
}

/// Index-based grouping (current behaviour): for each index k, take pose k from each snapshot.
pub fn group_poses_by_index(
    snapshots: &[crate::grpc::proto::Snapshot],
    camera_matrices: &[Matrix3x4<f64>],
) -> Vec<MatchedGroup> {
    let n = snapshots.iter().map(|s| s.poses.len()).min().unwrap_or(0);
    let num_cams = snapshots.len();
    if n == 0 || num_cams == 0 || camera_matrices.len() != num_cams {
        return vec![];
    }
    (0..n)
        .map(|k| {
            let row: Vec<Option<Pose2D>> = snapshots
                .iter()
                .map(|s| s.poses.get(k).cloned())
                .collect();
            MatchedGroup(row)
        })
        .collect()
}

/// Match poses across 2 or 3 cameras. Uses reprojection-based matching for 3 cams;
/// for 2 cams uses index-based. Falls back to index-based if 3-cam matching returns no groups.
pub fn match_poses(
    snapshots: &[crate::grpc::proto::Snapshot],
    camera_matrices: &[Matrix3x4<f64>],
    threshold: f64,
) -> Vec<MatchedGroup> {
    if snapshots.len() != camera_matrices.len() {
        return vec![];
    }
    match snapshots.len() {
        0 | 1 => vec![],
        2 => group_poses_by_index(snapshots, camera_matrices),
        3 => {
            let matched = match_poses_3cam(snapshots, camera_matrices, threshold);
            if matched.is_empty() {
                log::debug!("Pose matching: no groups below threshold, falling back to index-based");
                group_poses_by_index(snapshots, camera_matrices)
            } else {
                matched
            }
        }
        _ => {
            log::debug!("Pose matching: >3 cameras not implemented, using index-based");
            group_poses_by_index(snapshots, camera_matrices)
        }
    }
}

/// Triangulate each matched group and return 3D poses. Skips groups with <2 views.
pub fn triangulate_matched_groups(
    groups: &[MatchedGroup],
    camera_matrices: &[Matrix3x4<f64>],
) -> Vec<Result<Pose3D, HubError>> {
    groups
        .iter()
        .filter_map(|g| {
            g.to_poses_and_matrices(camera_matrices)
                .map(|(poses, matrices)| {
                    triangulate_from_poses_and_camera_matrices_partial(&poses, &matrices)
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::{CalibrationParameters, CameraExtrinsics, CameraIdentifier, CameraIntrinsics, Point2D, Snapshot};
    use super::super::calculate_camera_matrix;

    fn make_calibration(view_matrix: Vec<f64>) -> CalibrationParameters {
        CalibrationParameters {
            intrinsics: Some(CameraIntrinsics {
                camera_matrix: vec![500.0, 0.0, 320.0, 0.0, 500.0, 240.0, 0.0, 0.0, 1.0],
                distortion: vec![0.0; 5],
                rms_error: 0.0,
            }),
            extrinsics: Some(CameraExtrinsics { view_matrix }),
        }
    }

    fn full_pose2d(x: f64, y: f64, score: f64) -> Pose2D {
        let pt = Point2D { x, y, score };
        Pose2D {
            nose: Some(pt.clone()),
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
            score,
        }
    }

    fn snapshot_with_poses(which_camera: Option<CameraIdentifier>, poses: Vec<Pose2D>) -> Snapshot {
        Snapshot {
            timestamp: None,
            which_camera,
            poses,
            image: None,
        }
    }

    #[test]
    fn test_hungarian_square() {
        let cost = vec![
            vec![1.0, 2.0, 3.0],
            vec![2.0, 1.0, 2.0],
            vec![3.0, 2.0, 1.0],
        ];
        let a = hungarian_min_cost(&cost);
        assert_eq!(a.len(), 3);
        let cost_sum: f64 = a.iter().map(|(i, j)| cost[*i][*j]).sum();
        assert!((cost_sum - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_hungarian_rectangular() {
        let cost = vec![vec![1.0, 10.0], vec![10.0, 1.0]];
        let a = hungarian_min_cost(&cost);
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn test_group_poses_by_index_two_cams() {
        let id0 = CameraIdentifier { group_name: "g".into(), camera_name: "c0".into() };
        let id1 = CameraIdentifier { group_name: "g".into(), camera_name: "c1".into() };
        let s0 = snapshot_with_poses(Some(id0), vec![full_pose2d(0.0, 0.0, 1.0), full_pose2d(1.0, 1.0, 1.0)]);
        let s1 = snapshot_with_poses(Some(id1), vec![full_pose2d(2.0, 2.0, 1.0), full_pose2d(3.0, 3.0, 1.0)]);
        let cal = make_calibration(vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let p = calculate_camera_matrix(&cal).unwrap();
        let snapshots = vec![s0, s1];
        let matrices = vec![p, p];
        let groups = group_poses_by_index(&snapshots, &matrices);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].view_count(), 2);
        assert_eq!(groups[1].view_count(), 2);
    }

    #[test]
    fn test_match_poses_two_cams_uses_index() {
        let id0 = CameraIdentifier { group_name: "g".into(), camera_name: "c0".into() };
        let id1 = CameraIdentifier { group_name: "g".into(), camera_name: "c1".into() };
        let s0 = snapshot_with_poses(Some(id0), vec![full_pose2d(0.0, 0.0, 1.0)]);
        let s1 = snapshot_with_poses(Some(id1), vec![full_pose2d(2.0, 2.0, 1.0)]);
        let cal = make_calibration(vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let p = calculate_camera_matrix(&cal).unwrap();
        let groups = match_poses(&[s0, s1], &[p, p], 100.0);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].view_count(), 2);
    }

    #[test]
    fn test_match_poses_three_cams_consistent_order() {
        let id0 = CameraIdentifier { group_name: "g".into(), camera_name: "c0".into() };
        let id1 = CameraIdentifier { group_name: "g".into(), camera_name: "c1".into() };
        let id2 = CameraIdentifier { group_name: "g".into(), camera_name: "c2".into() };
        let cal0 = make_calibration(vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let cal1 = make_calibration(vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let cal2 = make_calibration(vec![1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let p0 = calculate_camera_matrix(&cal0).unwrap();
        let p1 = calculate_camera_matrix(&cal1).unwrap();
        let p2 = calculate_camera_matrix(&cal2).unwrap();
        let pose0 = full_pose2d(320.0, 240.0, 1.0);
        let pose1 = full_pose2d(330.0, 240.0, 1.0);
        let pose2 = full_pose2d(325.0, 240.0, 1.0);
        let s0 = snapshot_with_poses(Some(id0), vec![pose0]);
        let s1 = snapshot_with_poses(Some(id1), vec![pose1]);
        let s2 = snapshot_with_poses(Some(id2), vec![pose2]);
        let snapshots = vec![s0, s1, s2];
        let matrices = vec![p0, p1, p2];
        let groups = match_poses_3cam(&snapshots, &matrices, 50.0);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].view_count(), 3);
    }

    #[test]
    fn test_match_poses_three_cams_swapped_order() {
        let id0 = CameraIdentifier { group_name: "g".into(), camera_name: "c0".into() };
        let id1 = CameraIdentifier { group_name: "g".into(), camera_name: "c1".into() };
        let id2 = CameraIdentifier { group_name: "g".into(), camera_name: "c2".into() };
        let cal0 = make_calibration(vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let cal1 = make_calibration(vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let cal2 = make_calibration(vec![1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let p0 = calculate_camera_matrix(&cal0).unwrap();
        let p1 = calculate_camera_matrix(&cal1).unwrap();
        let p2 = calculate_camera_matrix(&cal2).unwrap();
        let pa = full_pose2d(320.0, 240.0, 1.0);
        let pb = full_pose2d(330.0, 240.0, 1.0);
        let pc = full_pose2d(325.0, 240.0, 1.0);
        let pd = full_pose2d(100.0, 100.0, 1.0);
        let pe = full_pose2d(110.0, 100.0, 1.0);
        let pf = full_pose2d(105.0, 100.0, 1.0);
        let s0 = snapshot_with_poses(Some(id0), vec![pa.clone(), pd.clone()]);
        let s1 = snapshot_with_poses(Some(id1), vec![pe.clone(), pb.clone()]);
        let s2 = snapshot_with_poses(Some(id2), vec![pf.clone(), pc.clone()]);
        let snapshots = vec![s0, s1, s2];
        let matrices = vec![p0, p1, p2];
        let groups = match_poses_3cam(&snapshots, &matrices, 50.0);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].view_count(), 3);
        assert_eq!(groups[1].view_count(), 3);
    }

    #[test]
    fn test_triangulate_matched_groups_yields_pose3d() {
        let cal0 = make_calibration(vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let cal1 = make_calibration(vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let cal2 = make_calibration(vec![1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let p0 = calculate_camera_matrix(&cal0).unwrap();
        let p1 = calculate_camera_matrix(&cal1).unwrap();
        let p2 = calculate_camera_matrix(&cal2).unwrap();
        let pose0 = full_pose2d(320.0, 240.0, 1.0);
        let pose1 = full_pose2d(330.0, 240.0, 1.0);
        let pose2 = full_pose2d(325.0, 240.0, 1.0);
        let group = MatchedGroup(vec![Some(pose0), Some(pose1), Some(pose2)]);
        let matrices = vec![p0, p1, p2];
        let results = triangulate_matched_groups(&[group], &matrices);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
        let pose3d = results[0].as_ref().unwrap();
        assert!(pose3d.nose.is_some());
    }

    #[test]
    fn test_match_poses_fallback_to_index_when_empty() {
        let id0 = CameraIdentifier { group_name: "g".into(), camera_name: "c0".into() };
        let id1 = CameraIdentifier { group_name: "g".into(), camera_name: "c1".into() };
        let id2 = CameraIdentifier { group_name: "g".into(), camera_name: "c2".into() };
        let cal0 = make_calibration(vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let cal1 = make_calibration(vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let cal2 = make_calibration(vec![1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let p0 = calculate_camera_matrix(&cal0).unwrap();
        let p1 = calculate_camera_matrix(&cal1).unwrap();
        let p2 = calculate_camera_matrix(&cal2).unwrap();
        let pose0 = full_pose2d(320.0, 240.0, 1.0);
        let pose1 = full_pose2d(330.0, 240.0, 1.0);
        let pose2 = full_pose2d(325.0, 240.0, 1.0);
        let s0 = snapshot_with_poses(Some(id0), vec![pose0]);
        let s1 = snapshot_with_poses(Some(id1), vec![pose1]);
        let s2 = snapshot_with_poses(Some(id2), vec![pose2]);
        let snapshots = vec![s0, s1, s2];
        let matrices = vec![p0, p1, p2];
        let groups = match_poses(&snapshots, &matrices, 0.0);
        assert!(!groups.is_empty(), "with threshold 0 all pairs rejected, should fall back to index-based");
        assert_eq!(groups.len(), 1);
    }
}
