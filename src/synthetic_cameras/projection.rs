//! Project 3D poses to 2D for each synthetic camera.

use std::convert::TryInto;

use crate::errors::CalculationError;
use crate::grpc::proto::{CalibrationParameters, Point2D, Pose2D, Pose3D};
use crate::openmvg::openmvg::get_projection;
use crate::triangulator::calculate_camera_matrix;

fn image_bounds_from_intrinsics(calibration: &CalibrationParameters) -> Option<(f64, f64)> {
    let k = calibration.intrinsics.as_ref()?.camera_matrix.as_slice();
    if k.len() < 6 {
        return None;
    }
    let cx = k[2];
    let cy = k[5];
    // Synthetic cameras assume principal point at image center.
    // Derive image size from that convention.
    let width = 2.0 * cx;
    let height = 2.0 * cy;
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        Some((width, height))
    } else {
        None
    }
}

fn point_in_frame(x: f64, y: f64, width: f64, height: f64) -> bool {
    x.is_finite() && y.is_finite() && x >= 0.0 && x <= width && y >= 0.0 && y <= height
}

/// Project a single 3D pose to 2D using the camera's full projection matrix P = K*[R|t].
pub fn project_pose3d_to_pose2d(
    pose3d: &Pose3D,
    calibration: &CalibrationParameters,
) -> Result<Pose2D, CalculationError> {
    let p = calculate_camera_matrix(calibration)?;
    let (points3d, score): (Vec<_>, f64) =
        pose3d.clone().try_into().map_err(CalculationError::from)?;
    let bounds = image_bounds_from_intrinsics(calibration);
    let mut points2d: Vec<Option<Point2D>> = Vec::with_capacity(17);
    for (pt, _s) in &points3d {
        let x2d = get_projection(*pt, p).map_err(CalculationError::from)?;
        let in_frame = bounds
            .map(|(w, h)| point_in_frame(x2d.x, x2d.y, w, h))
            .unwrap_or(true);
        if in_frame {
            points2d.push(Some(Point2D {
                x: x2d.x,
                y: x2d.y,
                score: 1.0,
            }));
        } else {
            points2d.push(None);
        }
    }
    Ok(Pose2D {
        nose: points2d[0].clone(),
        left_eye: points2d[1].clone(),
        right_eye: points2d[2].clone(),
        left_ear: points2d[3].clone(),
        right_ear: points2d[4].clone(),
        left_shoulder: points2d[5].clone(),
        right_shoulder: points2d[6].clone(),
        left_elbow: points2d[7].clone(),
        right_elbow: points2d[8].clone(),
        left_wrist: points2d[9].clone(),
        right_wrist: points2d[10].clone(),
        left_hip: points2d[11].clone(),
        right_hip: points2d[12].clone(),
        left_knee: points2d[13].clone(),
        right_knee: points2d[14].clone(),
        left_ankle: points2d[15].clone(),
        right_ankle: points2d[16].clone(),
        score,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic_cameras::camera_math::CameraConfig;
    use crate::synthetic_cameras::pose::points_to_pose3d;
    use nalgebra::Point3;

    #[test]
    fn project_pose3d_origin_through_centered_camera() {
        let config = CameraConfig {
            position: [0.0, 0.0, 5.0],
            look_at: [0.0, 0.0, 0.0],
            fov_deg: None,
            name: None,
            feed_path: None,
        };
        let cal = config.calibration();
        let points: Vec<_> = (0..17).map(|_| Point3::new(0.0, 0.0, 0.0)).collect();
        let pose3d = points_to_pose3d(&points, 1.0);
        let pose2d = project_pose3d_to_pose2d(&pose3d, &cal).expect("project");
        assert_eq!(pose2d.nose.as_ref().unwrap().x, 320.0);
        assert_eq!(pose2d.nose.as_ref().unwrap().y, 240.0);
    }

    #[test]
    fn project_pose3d_filters_points_outside_image_bounds() {
        // Use a known FOV-derived intrinsics (640x480 with 90° HFOV => fx=320).
        // A point far to the side should end up outside the image and be filtered out.
        let config = CameraConfig {
            position: [0.0, 0.0, 5.0],
            look_at: [0.0, 0.0, 0.0],
            fov_deg: Some(90.0),
            name: None,
            feed_path: None,
        };
        let cal = config.calibration();
        let points: Vec<_> = (0..17).map(|_| Point3::new(10.0, 0.0, 0.0)).collect();
        let pose3d = points_to_pose3d(&points, 1.0);
        let pose2d = project_pose3d_to_pose2d(&pose3d, &cal).expect("project");
        assert!(pose2d.nose.is_none(), "expected nose to be out of frame");
    }
}
