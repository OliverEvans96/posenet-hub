//! Project 3D poses to 2D for each synthetic camera.

use std::convert::TryInto;

use crate::errors::CalculationError;
use crate::errors::OpenMvgError;
use crate::grpc::proto::{CalibrationParameters, Point2D, Pose2D, Pose3D};
use crate::openmvg::openmvg::get_projection;
use crate::triangulator::calculate_camera_matrix;

/// Project a single 3D pose to 2D using the camera's full projection matrix P = K*[R|t].
pub fn project_pose3d_to_pose2d(
    pose3d: &Pose3D,
    calibration: &CalibrationParameters,
) -> Result<Pose2D, CalculationError> {
    let p = calculate_camera_matrix(calibration)?;
    let (points3d, score): (Vec<_>, f64) =
        pose3d.clone().try_into().map_err(CalculationError::from)?;
    let mut points2d = Vec::with_capacity(17);
    for (pt, _s) in &points3d {
        let x2d = get_projection(*pt, p).map_err(projection_error_to_calculation)?;
        points2d.push(Point2D {
            x: x2d.x,
            y: x2d.y,
            score: 1.0,
        });
    }
    Ok(Pose2D {
        nose: Some(points2d[0].clone()),
        left_eye: Some(points2d[1].clone()),
        right_eye: Some(points2d[2].clone()),
        left_ear: Some(points2d[3].clone()),
        right_ear: Some(points2d[4].clone()),
        left_shoulder: Some(points2d[5].clone()),
        right_shoulder: Some(points2d[6].clone()),
        left_elbow: Some(points2d[7].clone()),
        right_elbow: Some(points2d[8].clone()),
        left_wrist: Some(points2d[9].clone()),
        right_wrist: Some(points2d[10].clone()),
        left_hip: Some(points2d[11].clone()),
        right_hip: Some(points2d[12].clone()),
        left_knee: Some(points2d[13].clone()),
        right_knee: Some(points2d[14].clone()),
        left_ankle: Some(points2d[15].clone()),
        right_ankle: Some(points2d[16].clone()),
        score,
    })
}

fn projection_error_to_calculation(e: OpenMvgError) -> CalculationError {
    CalculationError::CameraMatrixFailed(crate::errors::MissingField::Keypoint(e.to_string()))
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
            name: None,
        };
        let cal = config.calibration();
        let points: Vec<_> = (0..17).map(|_| Point3::new(0.0, 0.0, 0.0)).collect();
        let pose3d = points_to_pose3d(&points, 1.0);
        let pose2d = project_pose3d_to_pose2d(&pose3d, &cal).expect("project");
        assert_eq!(pose2d.nose.as_ref().unwrap().x, 320.0);
        assert_eq!(pose2d.nose.as_ref().unwrap().y, 240.0);
    }
}
