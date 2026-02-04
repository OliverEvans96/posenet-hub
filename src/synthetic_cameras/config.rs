//! Configuration for run_synthetic_cameras.

use serde::Deserialize;
use std::path::PathBuf;

use super::camera_math::CameraConfig;

#[derive(Clone, Debug, Deserialize)]
pub struct SyntheticCamerasConfig {
    /// gRPC hub server URL (e.g. "http://127.0.0.1:50051").
    pub hub_url: String,
    /// Group name to register cameras under.
    pub group_name: String,
    /// Path to pose CSV (name, x, y, z).
    pub pose_csv_path: PathBuf,
    /// Camera definitions (position + look_at).
    pub cameras: Vec<CameraConfig>,
    /// Optional: run for this many seconds then exit. If None, run until interrupted.
    #[serde(default)]
    pub duration_secs: Option<u64>,
    /// Frames per second for streaming (default 30).
    #[serde(default = "default_fps")]
    pub fps: f32,
    /// Rotation speed: radians per second around Z axis (yaw; default 0.1).
    #[serde(default = "default_rotation_speed")]
    pub rotation_speed_rad_per_sec: f64,
}

fn default_fps() -> f32 {
    30.0
}

fn default_rotation_speed() -> f64 {
    0.1
}

impl SyntheticCamerasConfig {
    pub fn load_from_path(
        path: &std::path::Path,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let s = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&s)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_deserialize_defaults() {
        let yaml = r#"
hub_url: "http://127.0.0.1:50051"
group_name: "synthetic"
pose_csv_path: "fake_poses/running_pose.csv"
cameras:
  - position: [2, 1.5, 3]
    look_at: [0, 0, 0]
  - position: [-2, 1.5, 3]
    look_at: [0, 0, 0]
"#;
        let config: SyntheticCamerasConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.group_name, "synthetic");
        assert_eq!(config.cameras.len(), 2);
        assert_eq!(config.fps, 30.0);
        assert!(config.duration_secs.is_none());
        assert!((config.rotation_speed_rad_per_sec - 0.1).abs() < 1e-10);
    }

    #[test]
    fn config_deserialize_with_duration_and_fps() {
        let yaml = r#"
hub_url: "http://localhost:50051"
group_name: "test"
pose_csv_path: "pose.csv"
duration_secs: 60
fps: 15
rotation_speed_rad_per_sec: 0.2
cameras:
  - position: [0, 0, 5]
    look_at: [0, 0, 0]
"#;
        let config: SyntheticCamerasConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.duration_secs, Some(60));
        assert_eq!(config.fps, 15.0);
        assert!((config.rotation_speed_rad_per_sec - 0.2).abs() < 1e-10);
    }
}
