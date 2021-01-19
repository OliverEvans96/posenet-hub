use core::f64;

use nalgebra::{self, Matrix3x4};
use nalgebra::{Point2, Point3, Rotation3, Vector3};

use super::eigen::{Matrix3xN, ToEigen, ToNalgebra};

#[cxx::bridge]
mod ffi {
    #[namespace = "openMVG"]
    unsafe extern "C++" {
        type Mat3X = crate::openmvg::eigen::ffi::Mat3X;
        type Mat34 = crate::openmvg::eigen::ffi::Mat34;
        type Vec4 = crate::openmvg::eigen::ffi::Vec4;
    }

    unsafe extern "C++" {
        include!("posenet-vr-hub/include/openmvg.hpp");

        /// x's are landmark bearing vectors in each camera
        /// Ps are projective cameras
        fn triangulate_nview(
            x: UniquePtr<Mat3X>,
            Ps: UniquePtr<CxxVector<Mat34>>,
        ) -> UniquePtr<Vec4>;
    }
}

pub fn triangulate(points2d: &[Point2<f64>], camera_poses: &[Matrix3x4<f64>]) -> Point3<f64> {
    assert_eq!(points2d.len(), camera_poses.len());
    println!("Triangulating:");
    println!("Points: {:#?}", points2d);
    println!("Cameras: {:#?}", camera_poses);
    let x2d_h_mat = Matrix3xN::<f64>::from_columns(
        points2d
            .iter()
            .map(|x2d| x2d.to_homogeneous())
            .collect::<Vec<Vector3<f64>>>()
            .as_slice(),
    )
    .to_eigen();

    let camera_mat = camera_poses.to_eigen();

    let x3d_h_eig = ffi::triangulate_nview(x2d_h_mat, camera_mat);
    let x3d_h = x3d_h_eig.to_nalgebra();
    // TODO: Avoid copying? Does this `.into()` copy?
    let x3d =
        Point3::from_homogeneous(x3d_h.into()).expect("Triangulated point was not homogeneous");

    return x3d;
}

pub fn triangulate_many(
    points2d_slice: &[&[Point2<f64>]],
    camera_poses: &[Matrix3x4<f64>],
) -> Vec<Point3<f64>> {
    points2d_slice
        .iter()
        .map(|p2d| triangulate(p2d, camera_poses))
        .collect()
}

pub fn create_camera_matrix(center: Point3<f64>, rotation: Rotation3<f64>) -> Matrix3x4<f64> {
    // Convert to plain matrices
    let c = center.coords;
    let r = rotation.matrix();

    // Convert camera center to translation
    // See https://en.wikipedia.org/wiki/Camera_matrix#The_camera_position
    // and openMVG/src/openMVG/multiview/test_data_sets.cpp
    let t = -r * c;

    // Concatenate columns: p = [c | t]
    let mut cols: Vec<_> = r.column_iter().collect();
    cols.push(t.column(0));
    Matrix3x4::<f64>::from_columns(cols.as_slice())
}

/// Project a single 3D point onto a single camera
pub fn get_projection(x3d: Point3<f64>, p: Matrix3x4<f64>) -> Point2<f64> {
    let x3d_h = x3d.to_homogeneous();
    let x2d_h = p * x3d_h;
    let x2d = Point2::<f64>::from_homogeneous(x2d_h);
    x2d.expect("Point was not homogeneous, projection failed.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::{Point2, Point3, Vector2};

    #[test]
    fn test_projection() {
        let c = Point3::<f64>::new(1.0, 0.9, 0.1);
        let r = Rotation3::from_euler_angles(0.0, 0.1, 0.2);
        let p = create_camera_matrix(c, r);
        let x3d = Point3::<f64>::new(1.0, -1.0, 2.0);
        let x2d = get_projection(x3d, p);
        println!("c = {}", c);
        println!("r = {}", r.matrix());
        println!("x3d = {}", x3d);
        println!("x2d = {}", x2d);
    }

    #[test]
    fn test_rand_triangulate() {
        let nviews = 5;
        let mut points2d = Vec::<Point2<f64>>::with_capacity(nviews);
        let mut camera_poses = Vec::<Matrix3x4<f64>>::with_capacity(nviews);

        for _ in 0..nviews {
            let point2d = Point2::from(Vector2::new_random());
            let camera_pose = Matrix3x4::new_random();
            points2d.push(point2d);
            camera_poses.push(camera_pose);
        }

        let x3d: Point3<f64> = triangulate(points2d.as_slice(), camera_poses.as_mut_slice());
        println!("RAND x3d = {}", x3d);
    }

    #[test]
    fn test_triangulate() {
        // let d = ffi::create_nview_dataset(3, 4);
        let nviews = 2;
        let npoints = 3;

        let tol = 1e-9;

        // Create 3D point and cameras
        let mut p_vec: Vec<Matrix3x4<f64>> = (0..nviews).map(|_| Matrix3x4::new_random()).collect();

        for _ in 0..npoints {
            // Create 3D point
            let x3d: Point3<_> = Vector3::<f64>::new_random().into();
            // Project onto each camera
            let x2d_vec: Vec<_> = p_vec.iter().map(|&p| get_projection(x3d, p)).collect();
            // Reconstruct
            let x3d_recon = triangulate(x2d_vec.as_slice(), &mut p_vec);
            let x3d_recon_h = x3d_recon.to_homogeneous();
            // Compare reconstruction with original
            assert!((x3d - x3d_recon).norm() < tol);

            for j in 0..nviews {
                // Reproject reconstructed point to each camera
                let p = p_vec[j];
                let x2d = x2d_vec[j];
                let x2d_reproj_h = p * x3d_recon_h;
                let x2d_reproj = Point2::<f64>::from_homogeneous(x2d_reproj_h)
                    .expect("Reprojected 2D point was not homogeneous");
                // Compare reprojection with original projection
                assert!((x2d - x2d_reproj).norm() < tol);
            }
        }
    }
}
