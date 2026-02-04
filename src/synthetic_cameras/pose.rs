//! Load 3D poses from CSV (name, x, y, z) and apply rotations.

use nalgebra::{Point3, Rotation3, Vector3};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

use crate::grpc::proto::{Pose3D, POSE_KEYPOINTS};

#[derive(Error, Debug)]
pub enum PoseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("CSV parse error: {0}")]
    Csv(#[from] csv::Error),
    #[error("Missing keypoint in CSV: {0}")]
    MissingKeypoint(String),
    #[error("Duplicate keypoint in CSV: {0}")]
    DuplicateKeypoint(String),
}

/// Load a single 3D pose from a CSV file with columns: name, x, y, z.
/// Keypoint names must match POSE_KEYPOINTS; order in the file is ignored.
pub fn load_pose_csv(path: &Path) -> Result<Vec<Point3<f64>>, PoseError> {
    let mut reader = csv::Reader::from_path(path)?;
    let mut map: HashMap<String, Point3<f64>> = HashMap::new();
    for result in reader.deserialize() {
        let row: (String, f64, f64, f64) = result?;
        let (name, x, y, z) = row;
        let name = name.trim().to_lowercase();
        if map.insert(name.clone(), Point3::new(x, y, z)).is_some() {
            return Err(PoseError::DuplicateKeypoint(name));
        }
    }
    let mut points = Vec::with_capacity(17);
    for name in POSE_KEYPOINTS {
        let p = map
            .get(name)
            .ok_or_else(|| PoseError::MissingKeypoint(name.to_string()))?;
        points.push(*p);
    }
    Ok(points)
}

/// Convert slice of 17 Points to Pose3D with default score 1.0.
pub fn points_to_pose3d(points: &[Point3<f64>], score: f64) -> Pose3D {
    assert_eq!(points.len(), 17);
    Pose3D {
        nose: Some(point_to_proto(points[0])),
        left_eye: Some(point_to_proto(points[1])),
        right_eye: Some(point_to_proto(points[2])),
        left_ear: Some(point_to_proto(points[3])),
        right_ear: Some(point_to_proto(points[4])),
        left_shoulder: Some(point_to_proto(points[5])),
        right_shoulder: Some(point_to_proto(points[6])),
        left_elbow: Some(point_to_proto(points[7])),
        right_elbow: Some(point_to_proto(points[8])),
        left_wrist: Some(point_to_proto(points[9])),
        right_wrist: Some(point_to_proto(points[10])),
        left_hip: Some(point_to_proto(points[11])),
        right_hip: Some(point_to_proto(points[12])),
        left_knee: Some(point_to_proto(points[13])),
        right_knee: Some(point_to_proto(points[14])),
        left_ankle: Some(point_to_proto(points[15])),
        right_ankle: Some(point_to_proto(points[16])),
        score,
    }
}

fn point_to_proto(p: Point3<f64>) -> crate::grpc::proto::Point3D {
    crate::grpc::proto::Point3D {
        x: p.x,
        y: p.y,
        z: p.z,
        score: 1.0,
    }
}

/// Apply a rotation around the given center to a slice of 3D points (in place).
///
/// **Important:** For animation, always rotate from the *original* points each frame using
/// a single angle (e.g. `angle = speed * elapsed_time`). Do not accumulate angle and
/// re-apply to the same buffer each frame: that composes rotations and makes effective
/// angle grow as 1+2+...+n = n(n+1)/2 per frame n, so rotation appears to speed up
/// (quadratically) and can look like it reverses when the angle wraps.
pub fn rotate_pose_around(
    points: &mut [Point3<f64>],
    center: Point3<f64>,
    rotation: &Rotation3<f64>,
) {
    for p in points.iter_mut() {
        let translated: Vector3<f64> = p.coords - center.coords;
        let rotated: Vector3<f64> = rotation * translated;
        *p = Point3::from(center.coords + rotated);
    }
}

/// Rotation around vertical (Y) axis for given angle in radians.
#[allow(dead_code)]
pub fn rotation_around_y_rad(angle_rad: f64) -> Rotation3<f64> {
    Rotation3::from_euler_angles(0.0, angle_rad, 0.0)
}

/// Rotation around Z axis (yaw: horizontal plane) for given angle in radians.
/// Use this for a subject spinning in place (constant rate with elapsed time).
pub fn rotation_around_z_rad(angle_rad: f64) -> Rotation3<f64> {
    Rotation3::from_euler_angles(0.0, 0.0, angle_rad)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn load_pose_csv_parses_running_pose_order() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fake_poses/running_pose.csv");
        let points = load_pose_csv(&path).expect("load running_pose.csv");
        assert_eq!(points.len(), 17);
        // Nose first
        assert!((points[0].x - (-2.461178)).abs() < 1e-5);
        assert!((points[0].y - (-1.480207)).abs() < 1e-5);
        assert!((points[0].z - 4.222726).abs() < 1e-5);
        // right_ankle last
        assert!((points[16].x - (-1.478966)).abs() < 1e-5);
        assert!((points[16].z - 1.049240).abs() < 1e-5);
    }

    #[test]
    fn load_pose_csv_fails_on_missing_keypoint() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "name,x,y,z").unwrap();
        writeln!(f, "nose,0,0,0").unwrap();
        let path = f.path();
        let err = load_pose_csv(path).unwrap_err();
        assert!(matches!(err, PoseError::MissingKeypoint(_)));
    }

    #[test]
    fn load_pose_csv_fails_on_duplicate_keypoint() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "name,x,y,z").unwrap();
        writeln!(f, "nose,0,0,0").unwrap();
        writeln!(f, "nose,1,1,1").unwrap();
        let path = f.path();
        let err = load_pose_csv(path).unwrap_err();
        assert!(matches!(err, PoseError::DuplicateKeypoint(_)));
    }

    #[test]
    fn points_to_pose3d_has_17_keypoints() {
        let points: Vec<_> = (0..17)
            .map(|i| Point3::new(i as f64, i as f64, i as f64))
            .collect();
        let pose = points_to_pose3d(&points, 0.9);
        assert_eq!(pose.nose.unwrap().x, 0.0);
        assert_eq!(pose.right_ankle.unwrap().z, 16.0);
        assert_eq!(pose.score, 0.9);
    }

    #[test]
    fn rotate_pose_around_y_preserves_center() {
        let center = Point3::new(1.0, 2.0, 3.0);
        let mut points = vec![
            center,
            Point3::new(2.0, 2.0, 3.0),
            Point3::new(1.0, 3.0, 3.0),
        ];
        let rot = rotation_around_y_rad(std::f64::consts::FRAC_PI_2);
        rotate_pose_around(&mut points, center, &rot);
        assert!((points[0].x - center.x).abs() < 1e-10);
        assert!((points[0].y - center.y).abs() < 1e-10);
        assert!((points[0].z - center.z).abs() < 1e-10);
    }
}
