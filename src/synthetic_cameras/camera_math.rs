//! Camera position/orientation and calibration for synthetic cameras.

use nalgebra::{Matrix3, Matrix3x4, Point3, Rotation3, Vector3};
use serde::Deserialize;

use crate::grpc::proto::{CalibrationParameters, CameraExtrinsics, CameraIntrinsics};

/// Default focal length and principal point for synthetic pinhole cameras.
const DEFAULT_FX: f64 = 500.0;
const DEFAULT_FY: f64 = 500.0;
const DEFAULT_CX: f64 = 320.0;
const DEFAULT_CY: f64 = 240.0;

/// Default synthetic image size used for intrinsics derived from FOV.
const DEFAULT_IMAGE_WIDTH_PX: f64 = 640.0;
const DEFAULT_IMAGE_HEIGHT_PX: f64 = 480.0;

/// Camera position and orientation in config (YAML).
#[derive(Clone, Debug, Deserialize)]
pub struct CameraConfig {
    /// Camera position in world (meters).
    pub position: [f64; 3],
    /// Point the camera looks at in world (meters). Used to derive orientation.
    pub look_at: [f64; 3],
    /// Horizontal field of view (degrees). If present, intrinsics are derived from this.
    #[serde(default)]
    pub fov_deg: Option<f64>,
    /// Optional display name for this camera.
    #[serde(default)]
    pub name: Option<String>,
}

/// Build a 3x4 [R|t] extrinsics matrix: world to camera.
/// Camera at `position`, looking at `look_at`. World up = +Z.
/// OpenMVG convention: t = -R*C, so P = [R|t] with C = camera center in world.
pub fn view_matrix_from_position_look_at(
    position: Point3<f64>,
    look_at: Point3<f64>,
) -> Matrix3x4<f64> {
    let forward = (look_at - position).normalize();
    // World up for this project is +Z.
    // If forward is near-parallel with the up axis (camera directly above/below target),
    // fall back to a different up axis to avoid degeneracy.
    let world_up = Vector3::new(0.0, 0.0, 1.0);
    let alt_up = Vector3::new(0.0, 1.0, 0.0);
    let up_used = if forward.dot(&world_up).abs() > 0.999 {
        alt_up
    } else {
        world_up
    };
    let right = forward.cross(&up_used).normalize();
    // Use forward.cross(right) so that det[R] = +1 (right-handed camera frame).
    let up = forward.cross(&right).normalize();
    // OpenMVG/triangulator: R is world-to-camera, so ROWS of R are the camera axes in world
    // (row 0 = right, row 1 = up, row 2 = forward). Projection depth = row 2 · (x - C).
    // Triangulator extracts right/up/forward as first/second/third row of stored R.
    let r = Rotation3::from_matrix(&Matrix3::from_row_slice(&[
        right.x, right.y, right.z,
        up.x, up.y, up.z,
        forward.x, forward.y, forward.z,
    ]));
    crate::openmvg::openmvg::create_camera_matrix(position, r)
}

/// Flatten 3x4 matrix to row-major vec (12 elements) for proto view_matrix.
pub fn matrix3x4_to_view_matrix_vec(p: &Matrix3x4<f64>) -> Vec<f64> {
    let mut v = Vec::with_capacity(12);
    for i in 0..3 {
        for j in 0..4 {
            v.push(p[(i, j)]);
        }
    }
    v
}

/// Build calibration parameters for a synthetic camera.
pub fn calibration_from_view_and_intrinsics(
    view_matrix: &Matrix3x4<f64>,
    fx: f64,
    fy: f64,
    cx: f64,
    cy: f64,
) -> CalibrationParameters {
    let intrinsics = CameraIntrinsics {
        camera_matrix: vec![fx, 0.0, cx, 0.0, fy, cy, 0.0, 0.0, 1.0],
        distortion: vec![0.0, 0.0, 0.0, 0.0, 0.0],
        rms_error: 0.0,
    };
    let extrinsics = CameraExtrinsics {
        view_matrix: matrix3x4_to_view_matrix_vec(view_matrix),
    };
    CalibrationParameters {
        intrinsics: Some(intrinsics),
        extrinsics: Some(extrinsics),
    }
}

impl CameraConfig {
    /// Camera position as Point3.
    pub fn position_point(&self) -> Point3<f64> {
        Point3::new(self.position[0], self.position[1], self.position[2])
    }

    /// Look-at target as Point3.
    pub fn look_at_point(&self) -> Point3<f64> {
        Point3::new(self.look_at[0], self.look_at[1], self.look_at[2])
    }

    /// View matrix [R|t] for this camera (extrinsics only).
    pub fn view_matrix(&self) -> Matrix3x4<f64> {
        view_matrix_from_position_look_at(self.position_point(), self.look_at_point())
    }

    /// Full calibration (intrinsics + extrinsics) using default intrinsics.
    pub fn calibration(&self) -> CalibrationParameters {
        // If FOV is provided, derive focal length assuming a pinhole camera model with a
        // fixed synthetic image size.
        if let Some(fov_deg) = self.fov_deg {
            let fov_rad = fov_deg.to_radians();
            let fx = (DEFAULT_IMAGE_WIDTH_PX * 0.5) / (fov_rad * 0.5).tan();
            // Use square pixels for synthetic camera.
            let fy = fx;
            let cx = DEFAULT_IMAGE_WIDTH_PX * 0.5;
            let cy = DEFAULT_IMAGE_HEIGHT_PX * 0.5;
            return calibration_from_view_and_intrinsics(&self.view_matrix(), fx, fy, cx, cy);
        }
        calibration_from_view_and_intrinsics(
            &self.view_matrix(),
            DEFAULT_FX,
            DEFAULT_FY,
            DEFAULT_CX,
            DEFAULT_CY,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_matrix_camera_at_z_looking_at_origin() {
        let position = Point3::new(0.0, 0.0, 5.0);
        let look_at = Point3::new(0.0, 0.0, 0.0);
        let p = view_matrix_from_position_look_at(position, look_at);
        // Third column of R = camera Z axis in world. openMVG/nalgebra may use +Z or -Z
        // as viewing direction; either way origin (0,0,0) projects in front of camera at (0,0,5).
        let r = p.fixed_columns::<3>(0);
        let cam_z = r.column(2);
        let z_ok = (cam_z.x.abs() < 1e-9 && cam_z.y.abs() < 1e-9)
            && ((cam_z.z - 1.0).abs() < 1e-9 || (cam_z.z + 1.0).abs() < 1e-9);
        assert!(
            z_ok,
            "camera z should be (0,0,+1) or (0,0,-1): got ({}, {}, {})",
            cam_z.x, cam_z.y, cam_z.z
        );
    }

    #[test]
    fn camera_config_calibration_produces_12_element_view_matrix() {
        let config = CameraConfig {
            position: [2.0, 1.5, 3.0],
            look_at: [0.0, 0.0, 0.0],
            fov_deg: None,
            name: None,
        };
        let cal = config.calibration();
        let ext = cal.extrinsics.as_ref().unwrap();
        assert_eq!(ext.view_matrix.len(), 12);
        assert!(cal.intrinsics.is_some());
        let k = &cal.intrinsics.as_ref().unwrap().camera_matrix;
        assert_eq!(k[0], DEFAULT_FX);
        assert_eq!(k[4], DEFAULT_FY);
        assert_eq!(k[2], DEFAULT_CX);
        assert_eq!(k[5], DEFAULT_CY);
    }

    #[test]
    fn camera_config_fov_deg_derives_fx_from_default_width() {
        let config = CameraConfig {
            position: [2.0, 1.5, 3.0],
            look_at: [0.0, 0.0, 0.0],
            fov_deg: Some(90.0),
            name: None,
        };
        let cal = config.calibration();
        let k = &cal.intrinsics.as_ref().unwrap().camera_matrix;
        // For 90° horizontal FOV: fx = (w/2)/tan(45°) = w/2 = 320 for w=640.
        assert!((k[0] - 320.0).abs() < 1e-9, "fx expected ~320, got {}", k[0]);
        assert!((k[4] - 320.0).abs() < 1e-9, "fy expected ~320, got {}", k[4]);
        assert!((k[2] - 320.0).abs() < 1e-9, "cx expected 320, got {}", k[2]);
        assert!((k[5] - 240.0).abs() < 1e-9, "cy expected 240, got {}", k[5]);
    }

    #[test]
    fn matrix3x4_to_view_matrix_vec_row_major() {
        let p = Matrix3x4::new(
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0,
        );
        let v = matrix3x4_to_view_matrix_vec(&p);
        assert_eq!(v[0], 1.0);
        assert_eq!(v[3], 4.0);
        assert_eq!(v[4], 5.0);
        assert_eq!(v[11], 12.0);
    }
}
