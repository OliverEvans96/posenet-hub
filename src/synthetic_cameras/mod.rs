//! Synthetic cameras: stream fake pose observations from CSV poses projected through virtual cameras.

pub mod camera_math;
pub mod config;
pub mod pose;
pub mod projection;

pub use camera_math::{view_matrix_from_position_look_at, CameraConfig};
pub use config::SyntheticCamerasConfig;
pub use pose::{
    load_pose_csv, points_to_pose3d, rotate_pose_around, rotation_around_y_rad, PoseError,
};
pub use projection::project_pose3d_to_pose2d;
