//! Per-camera 2D pose smoothing with Kalman filters and temporal association.
//!
//! See `docs/pose-smoothing-design.md` for the full design. In short: we maintain
//! per-camera tracks (each track = 17 keypoint Kalman states), associate raw poses
//! to tracks by mean 2D keypoint distance, update with confidence-weighted Kalman,
//! and output smoothed Pose2D for matching, triangulation, and camera_views.

use rayon::prelude::*;
use std::collections::HashMap;

use crate::grpc::proto::{
    pose2d_from_partial_keypoints, pose2d_to_spoints_partial, Point2D, Pose2D, Snapshot,
};

/// Number of keypoints per pose (nose .. right_ankle).
const NUM_KEYPOINTS: usize = 17;

/// Score convention: 0 = bad, 1 = good. Used for measurement noise R and visibility.
const SCORE_EPSILON: f64 = 0.01;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Parameters for 2D pose smoothing (Kalman and association).
#[derive(Debug, Clone)]
pub struct SmoothingConfig {
    /// Base measurement variance (pixels²). r = R0 / max(score, epsilon).
    pub r0: f64,
    /// Minimum score in denominator to avoid division by zero.
    pub score_epsilon: f64,
    /// Process noise variance per dimension (position-only model).
    pub process_noise: f64,
    /// Max mean keypoint distance (px) to allow a raw–track match.
    pub assoc_threshold_px: f64,
    /// Min keypoint score to treat as "observed" (below ⇒ predict only).
    pub min_score_observed: f64,
    /// If 1: output last pose one more time when track unmatched then drop. If 0: drop immediately.
    pub hold_frames: u32,
}

impl Default for SmoothingConfig {
    fn default() -> Self {
        Self {
            r0: 100.0,
            score_epsilon: SCORE_EPSILON,
            process_noise: 4.0,
            assoc_threshold_px: 120.0,
            min_score_observed: 0.2,
            hold_frames: 1,
        }
    }
}

// -----------------------------------------------------------------------------
// Kalman 2D (position-only)
// -----------------------------------------------------------------------------

/// Per-keypoint 2D Kalman filter (position-only). State (x, y), diagonal covariance.
#[derive(Debug, Clone)]
pub struct Kalman2D {
    pub x: f64,
    pub y: f64,
    /// Variance per dimension (we use P = diag(p, p) for simplicity).
    pub p: f64,
}

impl Kalman2D {
    pub fn new(x: f64, y: f64, initial_variance: f64) -> Self {
        Self {
            x,
            y,
            p: initial_variance,
        }
    }

    /// Predict: state unchanged (position-only), add process noise.
    pub fn predict(&mut self, q: f64) {
        self.p += q;
    }

    /// Update with measurement (x, y) and score in [0, 1]. High score ⇒ trust measurement more.
    pub fn update(&mut self, z_x: f64, z_y: f64, score: f64, r0: f64, score_epsilon: f64) {
        let s = score.max(score_epsilon);
        let r = r0 / s;
        // Kalman gain (scalar per dimension): K = P / (P + R)
        let k = self.p / (self.p + r);
        self.x += k * (z_x - self.x);
        self.y += k * (z_y - self.y);
        self.p = (1.0 - k) * self.p;
    }

    pub fn position(&self) -> (f64, f64) {
        (self.x, self.y)
    }
}

// -----------------------------------------------------------------------------
// Pose2D track (17 keypoints + pose score)
// -----------------------------------------------------------------------------

/// One track: 17 keypoint Kalman states and pose-level score.
#[derive(Debug, Clone)]
pub struct Pose2DTrack {
    pub keypoints: [Kalman2D; NUM_KEYPOINTS],
    /// Smoothed or last-observed pose score.
    pub pose_score: f64,
    /// Last observed score per keypoint (for output); None if never observed.
    pub keypoint_scores: [Option<f64>; NUM_KEYPOINTS],
    /// Number of consecutive ticks this track was unmatched (for hold policy).
    pub unmatched_count: u32,
}

impl Pose2DTrack {
    /// Create a new track from the first observation (raw pose). No smoothing on first frame.
    pub fn from_pose(pose: &Pose2D, r0: f64, score_epsilon: f64) -> Self {
        let (opts, pose_score) = pose2d_to_spoints_partial(pose);
        let mut keypoints: [Kalman2D; NUM_KEYPOINTS] =
            std::array::from_fn(|_| Kalman2D::new(0.0, 0.0, r0));
        let mut keypoint_scores: [Option<f64>; NUM_KEYPOINTS] = std::array::from_fn(|_| None);
        for (i, opt) in opts.iter().enumerate() {
            if let Some((pt, score)) = opt {
                keypoints[i] = Kalman2D::new(pt.x, pt.y, r0 / score.max(score_epsilon));
                keypoint_scores[i] = Some(*score);
            }
        }
        Self {
            keypoints,
            pose_score,
            keypoint_scores,
            unmatched_count: 0,
        }
    }

    /// Predict all keypoints (call at start of tick).
    pub fn predict(&mut self, q: f64) {
        for k in &mut self.keypoints {
            k.predict(q);
        }
    }

    /// Update keypoints from a raw pose observation. Only keypoints with score >= min_score are updated.
    pub fn update(
        &mut self,
        raw: &Pose2D,
        config: &SmoothingConfig,
    ) {
        let (opts, raw_pose_score) = pose2d_to_spoints_partial(raw);
        for (i, opt) in opts.iter().enumerate() {
            if let Some((pt, score)) = opt {
                if *score >= config.min_score_observed {
                    self.keypoints[i].update(
                        pt.x,
                        pt.y,
                        *score,
                        config.r0,
                        config.score_epsilon,
                    );
                    self.keypoint_scores[i] = Some(*score);
                }
                // else: predict-only (already done); keep previous keypoint state and optionally decay score
            } else {
                // Missing: decay output score for this keypoint
                if let Some(s) = self.keypoint_scores[i].as_mut() {
                    *s *= 0.9;
                }
            }
        }
        // Pose score: use raw pose score (or EMA in future)
        self.pose_score = raw_pose_score;
        self.unmatched_count = 0;
    }

    /// Output smoothed Pose2D: positions from Kalman, scores from last observed or decayed.
    pub fn to_pose2d(&self) -> Pose2D {
        let keypoints: [Option<Point2D>; NUM_KEYPOINTS] =
            std::array::from_fn(|i| {
                let (x, y) = self.keypoints[i].position();
                let score = self.keypoint_scores[i].unwrap_or(0.0);
                Some(Point2D { x, y, score })
            });
        pose2d_from_partial_keypoints(keypoints, self.pose_score)
    }
}

// -----------------------------------------------------------------------------
// Association: mean keypoint distance
// -----------------------------------------------------------------------------

/// Cost between one raw pose and one track = mean 2D distance over visible keypoints.
/// Returns f64::INFINITY if fewer than min_visible keypoints overlap.
fn association_cost(
    raw: &Pose2D,
    track: &Pose2DTrack,
    min_score: f64,
    min_visible: usize,
) -> f64 {
    let (raw_opts, _) = pose2d_to_spoints_partial(raw);
    let mut sum = 0.0;
    let mut count = 0_usize;
    for i in 0..NUM_KEYPOINTS {
        let raw_pt = match &raw_opts[i] {
            Some((p, s)) if *s >= min_score => (p.x, p.y),
            _ => continue,
        };
        let (tx, ty) = track.keypoints[i].position();
        if track.keypoint_scores[i].is_some() {
            let dx = raw_pt.0 - tx;
            let dy = raw_pt.1 - ty;
            sum += (dx * dx + dy * dy).sqrt();
            count += 1;
        }
    }
    if count < min_visible {
        return f64::INFINITY;
    }
    sum / (count as f64)
}

/// Greedy one-to-one assignment: sort (raw_idx, track_idx) by cost, then assign in order.
/// Returns (raw_idx -> track_idx), unassigned raw indices, unassigned track indices.
fn assign_raw_to_tracks(
    raw_poses: &[Pose2D],
    tracks: &[Pose2DTrack],
    config: &SmoothingConfig,
) -> (
    Vec<Option<usize>>,
    Vec<usize>,
    Vec<usize>,
) {
    let n_raw = raw_poses.len();
    let n_track = tracks.len();
    if n_raw == 0 || n_track == 0 {
        return (
            (0..n_raw).map(|_| None).collect(),
            (0..n_raw).collect(),
            (0..n_track).collect(),
        );
    }
    let mut cost_pairs: Vec<(f64, usize, usize)> = Vec::with_capacity(n_raw * n_track);
    for (r, raw) in raw_poses.iter().enumerate() {
        for (t, track) in tracks.iter().enumerate() {
            let c = association_cost(raw, track, config.min_score_observed, 3);
            if c <= config.assoc_threshold_px {
                cost_pairs.push((c, r, t));
            }
        }
    }
    cost_pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut raw_to_track: Vec<Option<usize>> = vec![None; n_raw];
    let mut track_used = vec![false; n_track];
    for (_, r, t) in cost_pairs {
        if raw_to_track[r].is_none() && !track_used[t] {
            raw_to_track[r] = Some(t);
            track_used[t] = true;
        }
    }
    let unassigned_raw: Vec<usize> = (0..n_raw).filter(|&r| raw_to_track[r].is_none()).collect();
    let unassigned_track: Vec<usize> = (0..n_track).filter(|&t| !track_used[t]).collect();
    (raw_to_track, unassigned_raw, unassigned_track)
}

// -----------------------------------------------------------------------------
// Per-camera tracks
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Camera2DTracks {
    pub tracks: Vec<Pose2DTrack>,
    /// Next track id for stable ordering (optional; we use track index).
    _next_id: u64,
}

impl Camera2DTracks {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            _next_id: 0,
        }
    }

    /// One tick: associate raw poses to tracks, update matched, add new, drop unmatched (or hold).
    /// Returns list of smoothed Pose2D in stable order (matched in track order, then new).
    pub fn tick(&mut self, raw_poses: &[Pose2D], config: &SmoothingConfig) -> Vec<Pose2D> {
        if raw_poses.is_empty() {
            // Predict and optionally output held tracks
            for t in &mut self.tracks {
                t.predict(config.process_noise);
                t.unmatched_count += 1;
            }
            if config.hold_frames > 0 {
                return self
                    .tracks
                    .iter()
                    .filter(|t| t.unmatched_count <= config.hold_frames)
                    .map(|t| t.to_pose2d())
                    .collect();
            }
            self.tracks.retain(|t| t.unmatched_count == 0);
            return vec![];
        }

        // Predict all
        for t in &mut self.tracks {
            t.predict(config.process_noise);
        }

        let (raw_to_track, unassigned_raw, _unassigned_track) =
            assign_raw_to_tracks(raw_poses, &self.tracks, config);

        let mut out: Vec<Pose2D> = Vec::new();

        // Matched: update and output in track order (so order is stable)
        for (track_idx, track) in self.tracks.iter_mut().enumerate() {
            let raw_idx = raw_to_track.iter().position(|&ot| ot == Some(track_idx));
            if let Some(r) = raw_idx {
                track.update(&raw_poses[r], config);
                out.push(track.to_pose2d());
            } else {
                track.unmatched_count += 1;
                if config.hold_frames > 0 && track.unmatched_count <= config.hold_frames {
                    out.push(track.to_pose2d());
                }
            }
        }

        // New tracks from unassigned raw (first frame = raw, no smoothing)
        for &r in &unassigned_raw {
            let new_track = Pose2DTrack::from_pose(&raw_poses[r], config.r0, config.score_epsilon);
            out.push(raw_poses[r].clone()); // first frame: output raw
            self.tracks.push(new_track);
        }

        // Drop tracks that have been unmatched for longer than hold_frames
        self.tracks.retain(|t| t.unmatched_count <= config.hold_frames);

        out
    }
}

// -----------------------------------------------------------------------------
// Smooth snapshots (with Rayon over cameras)
// -----------------------------------------------------------------------------

/// Smooth 2D poses per camera and return snapshots with smoothed poses.
/// State is keyed by camera name; only the triangulator task should read/write it.
pub fn smooth_snapshots(
    current: &[Snapshot],
    state: &mut HashMap<String, Camera2DTracks>,
    config: &SmoothingConfig,
) -> Vec<Snapshot> {
    // Build (camera_name, raw_poses) for each snapshot; then run in parallel with copy-in/copy-out of tracks.
    let camera_names: Vec<String> = current
        .iter()
        .filter_map(|s| s.which_camera.as_ref().map(|w| w.camera_name.clone()))
        .collect();
    if camera_names.is_empty() {
        return current.to_vec();
    }

    // Copy out per-camera track state for parallel work
    let tracks_per_cam: Vec<(String, Camera2DTracks)> = camera_names
        .iter()
        .map(|name| (name.clone(), state.remove(name).unwrap_or_default()))
        .collect();

    let raw_poses_per_cam: Vec<Vec<Pose2D>> = current.iter().map(|s| s.poses.clone()).collect();

    // Parallel: each camera ticks and returns (camera_name, smoothed_poses, updated_tracks)
    let results: Vec<(String, Vec<Pose2D>, Camera2DTracks)> = tracks_per_cam
        .into_par_iter()
        .zip(raw_poses_per_cam.into_par_iter())
        .map(|((name, mut cam_tracks), raw_poses)| {
            let smoothed = cam_tracks.tick(&raw_poses, config);
            (name.clone(), smoothed, cam_tracks)
        })
        .collect();

    // Merge state back and build map name -> smoothed poses (order of current)
    let mut by_name: HashMap<String, (Vec<Pose2D>, Camera2DTracks)> = HashMap::new();
    for (name, smoothed, cam_tracks) in results {
        state.insert(name.clone(), cam_tracks.clone());
        by_name.insert(name, (smoothed, cam_tracks));
    }

    // Build smoothed snapshots in same order as current
    current
        .iter()
        .map(|snap| {
            let name = snap
                .which_camera
                .as_ref()
                .map(|w| w.camera_name.as_str())
                .unwrap_or("");
            let smoothed_poses = by_name
                .get(name)
                .map(|(p, _)| p.clone())
                .unwrap_or_else(|| snap.poses.clone());
            Snapshot {
                timestamp: snap.timestamp.clone(),
                which_camera: snap.which_camera.clone(),
                poses: smoothed_poses,
                image: snap.image.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::pose2d_to_spoints_partial;

    fn point2d(x: f64, y: f64, score: f64) -> Point2D {
        Point2D { x, y, score }
    }

    fn pose2d_with_all_keypoints(x: f64, y: f64, score: f64) -> Pose2D {
        let pts: [Option<Point2D>; 17] = [
            Some(point2d(x, y, score)),
            Some(point2d(x + 1.0, y, score)),
            Some(point2d(x - 1.0, y, score)),
            Some(point2d(x + 2.0, y, score)),
            Some(point2d(x - 2.0, y, score)),
            Some(point2d(x, y + 10.0, score)),
            Some(point2d(x, y + 10.0, score)),
            Some(point2d(x - 5.0, y + 15.0, score)),
            Some(point2d(x + 5.0, y + 15.0, score)),
            Some(point2d(x - 10.0, y + 20.0, score)),
            Some(point2d(x + 10.0, y + 20.0, score)),
            Some(point2d(x - 5.0, y + 30.0, score)),
            Some(point2d(x + 5.0, y + 30.0, score)),
            Some(point2d(x - 5.0, y + 45.0, score)),
            Some(point2d(x + 5.0, y + 45.0, score)),
            Some(point2d(x - 5.0, y + 60.0, score)),
            Some(point2d(x + 5.0, y + 60.0, score)),
        ];
        pose2d_from_partial_keypoints(pts, score)
    }

    #[test]
    fn kalman2d_predict_adds_uncertainty() {
        let mut k = Kalman2D::new(100.0, 200.0, 10.0);
        let p_before = k.p;
        k.predict(5.0);
        assert_eq!(k.x, 100.0);
        assert_eq!(k.y, 200.0);
        assert!(k.p > p_before);
    }

    #[test]
    fn kalman2d_update_pulls_toward_measurement() {
        let mut k = Kalman2D::new(100.0, 200.0, 100.0);
        k.update(110.0, 210.0, 1.0, 100.0, 0.01);
        assert!(k.x > 100.0 && k.x < 110.0);
        assert!(k.y > 200.0 && k.y < 210.0);
    }

    #[test]
    fn kalman2d_high_score_trusts_measurement_more() {
        let mut k1 = Kalman2D::new(0.0, 0.0, 100.0);
        let mut k2 = Kalman2D::new(0.0, 0.0, 100.0);
        k1.update(10.0, 10.0, 0.2, 100.0, 0.01);
        k2.update(10.0, 10.0, 1.0, 100.0, 0.01);
        assert!(k2.x > k1.x);
        assert!(k2.y > k1.y);
    }

    #[test]
    fn pose2d_track_from_pose_initializes_positions() {
        let pose = pose2d_with_all_keypoints(50.0, 60.0, 0.9);
        let config = SmoothingConfig::default();
        let track = Pose2DTrack::from_pose(&pose, config.r0, config.score_epsilon);
        assert_eq!(track.keypoints[0].x, 50.0);
        assert_eq!(track.keypoints[0].y, 60.0);
        assert_eq!(track.pose_score, 0.9);
    }

    #[test]
    fn pose2d_track_to_pose2d_roundtrip() {
        let pose = pose2d_with_all_keypoints(100.0, 200.0, 0.8);
        let config = SmoothingConfig::default();
        let track = Pose2DTrack::from_pose(&pose, config.r0, config.score_epsilon);
        let out = track.to_pose2d();
        let (opts, score) = pose2d_to_spoints_partial(&out);
        assert_eq!(score, 0.8);
        assert!(opts[0].is_some());
        let (p, _) = opts[0].as_ref().unwrap();
        assert!((p.x - 100.0).abs() < 1e-9);
        assert!((p.y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn camera_tracks_empty_raw_produces_empty_or_held() {
        let mut cam = Camera2DTracks::new();
        let config = SmoothingConfig { hold_frames: 0, ..Default::default() };
        let out = cam.tick(&[], &config);
        assert!(out.is_empty());
    }

    #[test]
    fn camera_tracks_first_frame_outputs_raw() {
        let mut cam = Camera2DTracks::new();
        let config = SmoothingConfig::default();
        let pose = pose2d_with_all_keypoints(10.0, 20.0, 1.0);
        let out = cam.tick(&[pose.clone()], &config);
        assert_eq!(out.len(), 1);
        let (opts, _) = pose2d_to_spoints_partial(&out[0]);
        assert!((opts[0].as_ref().unwrap().0.x - 10.0).abs() < 1e-5);
        assert_eq!(cam.tracks.len(), 1);
    }

    #[test]
    fn camera_tracks_second_frame_smooths() {
        let mut cam = Camera2DTracks::new();
        let config = SmoothingConfig::default();
        let p1 = pose2d_with_all_keypoints(100.0, 100.0, 1.0);
        let p2 = pose2d_with_all_keypoints(102.0, 101.0, 1.0); // small move
        cam.tick(&[p1], &config);
        let out = cam.tick(&[p2], &config);
        assert_eq!(out.len(), 1);
        let (opts, _) = pose2d_to_spoints_partial(&out[0]);
        let x = opts[0].as_ref().unwrap().0.x;
        let y = opts[0].as_ref().unwrap().0.y;
        assert!(x > 100.0 && x < 102.0);
        assert!(y > 100.0 && y < 101.0);
    }

    #[test]
    fn association_cost_same_pose_is_low() {
        let pose = pose2d_with_all_keypoints(50.0, 50.0, 1.0);
        let config = SmoothingConfig::default();
        let track = Pose2DTrack::from_pose(&pose, config.r0, config.score_epsilon);
        let c = association_cost(&pose, &track, config.min_score_observed, 3);
        assert!(c < 1.0);
    }

    #[test]
    fn association_cost_far_pose_is_high() {
        let p1 = pose2d_with_all_keypoints(0.0, 0.0, 1.0);
        let p2 = pose2d_with_all_keypoints(500.0, 500.0, 1.0);
        let config = SmoothingConfig::default();
        let track = Pose2DTrack::from_pose(&p1, config.r0, config.score_epsilon);
        let c = association_cost(&p2, &track, config.min_score_observed, 3);
        assert!(c > 100.0);
    }

    #[test]
    fn smooth_snapshots_preserves_structure() {
        let mut state = HashMap::new();
        let config = SmoothingConfig::default();
        let pose = pose2d_with_all_keypoints(40.0, 50.0, 0.9);
        let snap = Snapshot {
            timestamp: None,
            which_camera: Some(crate::grpc::proto::CameraIdentifier {
                group_name: "g".to_string(),
                camera_name: "cam0".to_string(),
            }),
            poses: vec![pose],
            image: None,
        };
        let smoothed = smooth_snapshots(&[snap.clone()], &mut state, &config);
        assert_eq!(smoothed.len(), 1);
        assert_eq!(smoothed[0].poses.len(), 1);
        assert!(smoothed[0].which_camera.as_ref().unwrap().camera_name == "cam0");
    }
}
