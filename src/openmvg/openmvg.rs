use core::f64;
use std::error::Error;
use std::iter::empty;

use nalgebra::{self, Matrix2xX, Matrix3, Matrix3x4, Matrix3xX, Matrix4, Vector2};
use nalgebra::{Point2, Point3, Rotation3, Vector3};

use super::eigen::{Matrix3xN, ToEigen, ToNalgebra};
use crate::grpc::proto::{CameraExtrinsics, CameraInfo, CameraIntrinsics, Point2D};

#[cxx::bridge]
mod ffi {
    #[namespace = "openMVG"]
    unsafe extern "C++" {
        type Mat2X = crate::openmvg::eigen::ffi::Mat2X;
        type Mat3X = crate::openmvg::eigen::ffi::Mat3X;
        type Mat34 = crate::openmvg::eigen::ffi::Mat34;
        type Mat3 = crate::openmvg::eigen::ffi::Mat3;
        type Vec3 = crate::openmvg::eigen::ffi::Vec3;
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

        fn ceres_bundle_adjustment(
            xs: UniquePtr<CxxVector<Mat2X>>,
            Ks: UniquePtr<CxxVector<Mat3>>,
            ts: UniquePtr<CxxVector<Vec3>>,
            Rs: UniquePtr<CxxVector<Mat3>>,
            x3d: UniquePtr<Mat3X>,
        ) -> bool;
    }
}

pub fn ceres_bundle_adjustment(
    points2d_slice: Vec<Vec<Point2<f64>>>,
    cameras: Vec<CameraInfo>,
) -> Option<bool> {
    // TODO: Major overhaul to this wrapper function
    let mut ks = Vec::new();
    let mut rs = Vec::new();
    let mut ts = Vec::new();
    // TODO: Get from args?
    let x3d = Matrix3xX::zeros(
        points2d_slice
            .first()
            .and_then(|f| Some(f.len()))
            .unwrap_or(0),
    );
    for camera in cameras {
        let k = Matrix3::<f64>::from_row_slice(&camera.intrinsics?.camera_matrix);
        let c = Matrix4::<f64>::from_row_slice(&camera.extrinsics?.view_matrix);
        // Remove bottom row (0 0 0 1)
        let cn = c.fixed_rows::<3>(0);
        let r: Matrix3<f64> = cn.fixed_columns::<3>(0).into();
        let t: Vector3<f64> = cn.fixed_columns::<1>(3).into();
        ks.push(k);
        rs.push(r);
        ts.push(t)
    }

    let xs: Vec<Matrix2xX<f64>> = points2d_slice
        .into_iter()
        .map(|points2d| {
            let columns: Vec<_> = points2d
                .iter()
                .flat_map(|point2d| point2d.coords.into_iter())
                .copied()
                .collect();
            Matrix2xX::from_column_slice(&columns)
        })
        .collect();

    Some(ffi::ceres_bundle_adjustment(
        xs.to_eigen(),
        ks.to_eigen(),
        ts.to_eigen(),
        rs.to_eigen(),
        x3d.to_eigen(),
    ))
}

/// Triangulate a single point across multiple cameras
pub fn triangulate(points2d: &[Point2<f64>], camera_poses: &[Matrix3x4<f64>]) -> Point3<f64> {
    assert_eq!(points2d.len(), camera_poses.len());
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

/// Triangulate many points (a single pose) across many cameras
pub fn triangulate_many<T>(
    points2d_slice: &[T],
    camera_poses: &[Matrix3x4<f64>],
) -> Vec<Point3<f64>>
where
    T: AsRef<[Point2<f64>]>,
{
    points2d_slice
        .iter()
        .map(|p2d| triangulate(p2d.as_ref(), camera_poses))
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
    use nalgebra::{Matrix3xX, Point2, Point3};
    use rand::distributions::Standard;

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
        use nalgebra::Vector2;
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

    #[test]
    fn test_rand_bundle_adjustment() {
        use rand::{thread_rng, Rng};

        use super::ceres_bundle_adjustment;

        let mut cameras = Vec::new();
        let mut points2d_slice = Vec::new();
        let nviews = 4;
        let npoints = 8;

        let rng = thread_rng();
        let mut rand_iter = rng.sample_iter(Standard);

        for i in 0..nviews {
            let extrinsics = CameraExtrinsics {
                view_matrix: (&mut rand_iter).take(16).collect(),
            };
            let intrinsics = CameraIntrinsics {
                camera_matrix: (&mut rand_iter).take(9).collect(),
                ..Default::default()
            };
            let camera = CameraInfo {
                extrinsics: Some(extrinsics),
                intrinsics: Some(intrinsics),
                ..Default::default()
            };
            cameras.push(camera);
            let mut points2d = Vec::new();
            for _ in 0..npoints {
                let coords: Vec<_> = (&mut rand_iter).take(2).collect();
                let point = Point2::from_slice(&coords);
                points2d.push(point);
            }
            points2d_slice.push(points2d);
        }

        let result = ceres_bundle_adjustment(points2d_slice, cameras).unwrap();
        assert_eq!(result, true);
    }

    #[test]
    fn test_real_pose_ceres_bundle_adjustment() {
        let nviews = 3;
        let npoints = 13;

        // Test data from snapshot 57179f56-0aa3-47d0-bd83-ee758996d02b
        // excluding legs because they weren't present in all views

        let mut xs = Vec::with_capacity(nviews);
        let mut rs = Vec::with_capacity(nviews);
        let mut ts = Vec::with_capacity(nviews);
        let mut ks = Vec::with_capacity(nviews);
        let x3d = Matrix3xX::zeros(npoints);

        // Camera 1
        {
            let x = Matrix2xX::from_row_slice(&vec![
                221.84979248,
                228.47775269,
                208.83682251,
                232.63215637,
                181.32733154,
                238.17430115,
                148.26821899,
                206.74200439,
                143.83721924,
                255.53611755,
                107.13804626,
                247.82676697,
                81.83927155,
                168.72930908,
                159.02880859,
                156.449646,
                164.5904541,
                159.87460327,
                231.87835693,
                221.94210815,
                383.17007446,
                368.08843994,
                307.41195679,
                293.77606201,
                381.96881104,
                369.11099243,
            ]);

            let r = Matrix3::from_row_slice(&vec![
                0.07629385,
                -0.92014996,
                0.38406159,
                -0.98778557,
                -0.1222358,
                -0.09663373,
                0.1358636,
                -0.37199794,
                -0.91823669,
            ]);

            let t = Vector3::from_column_slice(&vec![-0.61260745, 1.50300926, 8.23702323]);

            let k = Matrix3::from_row_slice(&vec![
                435.82994145,
                0.,
                308.13415848,
                0.,
                440.28175771,
                250.09298143,
                0.,
                0.,
                1.,
            ]);

            xs.push(x);
            rs.push(r);
            ts.push(t);
            ks.push(k);
        }

        // Camera 2
        {
            let x = Matrix2xX::from_row_slice(&vec![
                338.58163452,
                348.83416748,
                330.11764526,
                357.95767212,
                309.28704834,
                369.94433594,
                279.70275879,
                347.54989624,
                287.23718262,
                388.54776001,
                255.93977356,
                372.14831543,
                232.44750977,
                156.04859924,
                148.07814026,
                145.64706421,
                155.02645874,
                151.30648804,
                222.03936768,
                218.44535828,
                371.34475708,
                367.19726562,
                305.26754761,
                291.47875977,
                373.97061157,
                351.15512085,
            ]);

            let r = Matrix3::from_row_slice(&vec![
                0.14647675,
                -0.98582323,
                -0.0818359,
                -0.98336871,
                -0.13613183,
                -0.12022524,
                0.10738036,
                0.09808507,
                -0.98936787,
            ]);

            let t = Vector3::from_column_slice(&vec![-0.78149923, 1.73635204, 8.9053794]);

            let k = Matrix3::from_row_slice(&vec![
                448.5791123,
                0.,
                321.64924453,
                0.,
                448.98220527,
                227.00351986,
                0.,
                0.,
                1.,
            ]);

            xs.push(x);
            rs.push(r);
            ts.push(t);
            ks.push(k);
        }

        // Camera 3
        {
            let x = Matrix2xX::from_row_slice(&vec![
                529.58789062,
                541.94873047,
                519.27703857,
                558.08148193,
                502.14987183,
                597.44921875,
                491.42208862,
                592.17358398,
                510.73092651,
                632.23626709,
                472.46426392,
                593.02539062,
                452.65359497,
                159.5145874,
                147.74821472,
                148.13491821,
                153.43185425,
                153.69386292,
                228.62271118,
                234.00015259,
                402.11080933,
                394.72644043,
                321.4914856,
                307.12112427,
                406.79891968,
                367.63317871,
            ]);

            let r = Matrix3::from_row_slice(&vec![
                0.1200112,
                -0.698047,
                -0.70592329,
                -0.98938857,
                -0.02543588,
                -0.14304993,
                0.0818998,
                0.71560003,
                -0.69369231,
            ]);

            let t = Vector3::from_column_slice(&vec![0.89793293, 1.76919075, 10.16815631]);

            let k = Matrix3::from_row_slice(&vec![
                452.54818017,
                0.,
                309.75120253,
                0.,
                451.84428008,
                229.94379704,
                0.,
                0.,
                1.,
            ]);

            xs.push(x);
            rs.push(r);
            ts.push(t);
            ks.push(k);
        }

        let result = ffi::ceres_bundle_adjustment(
            xs.to_eigen(),
            rs.to_eigen(),
            ts.to_eigen(),
            ks.to_eigen(),
            x3d.to_eigen(),
        );

        // TODO: Use wrapper?
        // let result = ceres_bundle_adjustment(points2d_slice, cameras).unwrap();
        assert_eq!(result, true);
    }
}
