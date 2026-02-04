use core::f64;

use nalgebra::{self, Matrix2xX, Matrix3, Matrix3x4, Matrix3xX};
use nalgebra::{Point2, Point3, Rotation3, Vector3};

use super::eigen::{Matrix3xN, ToEigen, ToNalgebra};
use crate::errors::OpenMvgError;
use crate::grpc::proto::BundleAdjustmentOptions;

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

    struct BundleAdjustmentOptions {
        camera_rotation: bool,
        camera_translation: bool,
        camera_intrinsics: bool,
        pose3d: bool,
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
            xs: &UniquePtr<CxxVector<Mat2X>>,
            Ks: &mut UniquePtr<CxxVector<Mat3>>,
            ts: &mut UniquePtr<CxxVector<Vec3>>,
            Rs: &mut UniquePtr<CxxVector<Mat3>>,
            x3d: &mut UniquePtr<Mat3X>,
            opts: BundleAdjustmentOptions,
        ) -> bool;
    }
}

impl Default for ffi::BundleAdjustmentOptions {
    fn default() -> Self {
        Self {
            camera_rotation: true,
            camera_translation: true,
            camera_intrinsics: true,
            pose3d: true,
        }
    }
}

impl From<BundleAdjustmentOptions> for ffi::BundleAdjustmentOptions {
    fn from(opts: BundleAdjustmentOptions) -> Self {
        Self {
            camera_rotation: opts.camera_rotation,
            camera_translation: opts.camera_translation,
            camera_intrinsics: opts.camera_intrinsics,
            pose3d: opts.pose3d,
        }
    }
}

impl From<Option<BundleAdjustmentOptions>> for ffi::BundleAdjustmentOptions {
    fn from(maybe_opts: Option<BundleAdjustmentOptions>) -> Self {
        maybe_opts.map_or_else(Default::default, Into::into)
    }
}

pub fn ceres_bundle_adjustment(
    xs: &Vec<Matrix2xX<f64>>,
    ks: &mut Vec<Matrix3<f64>>,
    ts: &mut Vec<Vector3<f64>>,
    rs: &mut Vec<Matrix3<f64>>,
    x3d: &mut Matrix3xX<f64>,
    opts: Option<BundleAdjustmentOptions>,
) -> Result<bool, OpenMvgError> {
    let nviews = xs.len();

    let xse = xs.to_eigen();
    let mut rse = rs.to_eigen();
    let mut tse = ts.to_eigen();
    let mut kse = ks.to_eigen();
    let mut x3de = x3d.to_eigen();

    let result =
        ffi::ceres_bundle_adjustment(&xse, &mut kse, &mut tse, &mut rse, &mut x3de, opts.into());

    if result {
        let ksn = kse
            .to_nalgebra()
            .ok_or(OpenMvgError::FfiConversionFailed)?;
        let tsn = tse
            .to_nalgebra()
            .ok_or(OpenMvgError::FfiConversionFailed)?;
        let rsn = rse
            .to_nalgebra()
            .ok_or(OpenMvgError::FfiConversionFailed)?;
        let x3dn = x3de
            .to_nalgebra()
            .ok_or(OpenMvgError::FfiConversionFailed)?;

        x3d.copy_from(&x3dn);
        for i in 0..nviews {
            ks[i].copy_from(&ksn[i]);
            ts[i].copy_from(&tsn[i]);
            rs[i].copy_from(&rsn[i]);
        }
    }

    Ok(result)
}

/// Triangulate a single point across multiple cameras
pub fn triangulate(
    points2d: &[Point2<f64>],
    camera_poses: &[Matrix3x4<f64>],
) -> Result<Point3<f64>, OpenMvgError> {
    if points2d.len() != camera_poses.len() {
        return Err(OpenMvgError::DimensionMismatch {
            points: points2d.len(),
            cameras: camera_poses.len(),
        });
    }
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
    let x3d_h = x3d_h_eig
        .to_nalgebra()
        .ok_or(OpenMvgError::FfiConversionFailed)?;
    let x3d = Point3::from_homogeneous(x3d_h.into())
        .ok_or(OpenMvgError::NonHomogeneousPoint)?;

    Ok(x3d)
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
pub fn get_projection(
    x3d: Point3<f64>,
    p: Matrix3x4<f64>,
) -> Result<Point2<f64>, OpenMvgError> {
    let x3d_h = x3d.to_homogeneous();
    let x2d_h = p * x3d_h;
    Point2::<f64>::from_homogeneous(x2d_h).ok_or(OpenMvgError::NonHomogeneousPoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::{Matrix3xX, Point2, Point3};

    #[test]
    fn test_projection() {
        let c = Point3::<f64>::new(1.0, 0.9, 0.1);
        let r = Rotation3::from_euler_angles(0.0, 0.1, 0.2);
        let p = create_camera_matrix(c, r);
        let x3d = Point3::<f64>::new(1.0, -1.0, 2.0);
        let x2d = get_projection(x3d, p).unwrap();
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

        let x3d: Point3<f64> =
            triangulate(points2d.as_slice(), camera_poses.as_slice()).unwrap();
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
            let x2d_vec: Vec<_> = p_vec
                .iter()
                .map(|&p| get_projection(x3d, p).unwrap())
                .collect();
            // Reconstruct
            let x3d_recon = triangulate(x2d_vec.as_slice(), &p_vec).unwrap();
            let x3d_recon_h = x3d_recon.to_homogeneous();
            // Compare reconstruction with original
            assert!((x3d - x3d_recon).norm() < tol);

            for j in 0..nviews {
                // Reproject reconstructed point to each camera
                let p = p_vec[j];
                let x2d = x2d_vec[j];
                let x2d_reproj_h = p * x3d_recon_h;
                let x2d_reproj = Point2::<f64>::from_homogeneous(x2d_reproj_h)
                    .unwrap_or_else(|| panic!("Reprojected 2D point was not homogeneous"));
                // Compare reprojection with original projection
                assert!((x2d - x2d_reproj).norm() < tol);
            }
        }
    }

    fn real_pose_sfm_data() -> (
        Vec<Matrix2xX<f64>>,
        Vec<Matrix3<f64>>,
        Vec<Vector3<f64>>,
        Vec<Matrix3<f64>>,
        Matrix3xX<f64>,
    ) {
        let nviews = 3;
        // let npoints = 13;

        // Test data from snapshot 57179f56-0aa3-47d0-bd83-ee758996d02b
        // excluding legs because they weren't present in all views

        let mut xs = Vec::with_capacity(nviews);
        let mut rs = Vec::with_capacity(nviews);
        let mut ts = Vec::with_capacity(nviews);
        let mut ks = Vec::with_capacity(nviews);

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

        // Initial guess at 3d reconstruction
        let mut x3d = Matrix3xX::from_row_slice(&[
            5.75736038,
            6.19566462,
            6.31175113,
            6.04272584,
            6.11553653,
            3.5023089,
            2.95261357,
            -3.38746644,
            -2.73479096,
            0.20888902,
            0.08938635,
            -2.70167554,
            -2.65021819,
            -0.09247657,
            -0.38035375,
            0.3800269,
            -0.67891853,
            1.35480288,
            -1.49434688,
            2.17649122,
            -1.37799213,
            1.45635137,
            -2.53573721,
            3.15891996,
            -2.39838245,
            3.74450797,
            -9.03620081,
            -9.11342563,
            -9.55889736,
            -9.3307351,
            -10.1025985,
            -10.52251742,
            -8.39828106,
            -13.31026506,
            -10.19966323,
            -9.86394792,
            -9.9113146,
            -9.95466323,
            -10.43326768,
        ]);

        return (xs, rs, ts, ks, x3d);
    }

    use more_asserts::{assert_gt, assert_le};
    use rstest::rstest;

    /* TODO: Re-enable
    #[rstest]
    #[case(BundleAdjustmentOptions {
        camera_rotation: false,
        camera_translation: false,
        camera_intrinsics: false,
        pose3d: false,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: true,
        camera_translation: false,
        camera_intrinsics: false,
        pose3d: false,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: false,
        camera_translation: true,
        camera_intrinsics: false,
        pose3d: false,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: false,
        camera_translation: false,
        camera_intrinsics: true,
        pose3d: false,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: false,
        camera_translation: false,
        camera_intrinsics: false,
        pose3d: true,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: true,
        camera_translation: true,
        camera_intrinsics: true,
        pose3d: true,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: false,
        camera_translation: true,
        camera_intrinsics: true,
        pose3d: true,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: true,
        camera_translation: false,
        camera_intrinsics: true,
        pose3d: true,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: true,
        camera_translation: true,
        camera_intrinsics: false,
        pose3d: true,
    })]
    #[case(BundleAdjustmentOptions {
        camera_rotation: true,
        camera_translation: true,
        camera_intrinsics: true,
        pose3d: false,
    })]
    fn check_ba_yields_expected_changes(#[case] opts: BundleAdjustmentOptions) {
        let (xs, mut rs, mut ts, mut ks, mut x3d) = real_pose_sfm_data();

        // NOTE: OpenMVG's Pinhole Camera requires f = fx = fy, so it modifies K
        // immediately, even if nothing changes during the bundle adjustment.
        // To account for this, we'll make this change ahead of time.
        for k in ks.iter_mut() {
            let mean = (k[(0, 0)] + k[(1, 1)]) / 2.0;
            k[(0, 0)] = mean;
            k[(1, 1)] = mean;
        }

        // Copy values before mutating to compare later
        let ksc = ks.clone();
        let tsc = ts.clone();
        let rsc = rs.clone();
        let x3dc = x3d.clone();
        let optsc = opts.clone();

        // Perform BA
        let result = ceres_bundle_adjustment(&xs, &mut ks, &mut ts, &mut rs, &mut x3d, Some(opts))
            .expect("bundle adjustment should succeed");

        // The BA should succeed
        assert!(result);

        // Check that only the expected values changed
        let max_err = 1e-3;

        let err_x3d = (&x3dc - &x3d).norm() / x3dc.norm();
        if optsc.pose3d {
            assert_gt!(err_x3d, max_err);
            // assert_relative_eq!(x3dc, x3d, max_relative = max_relative);
        } else {
            assert_le!(err_x3d, max_err);
        }
        for (k, kc) in ks.iter().zip(ksc.iter()) {
            let err_k = (kc - k).norm() / kc.norm();
            if optsc.camera_intrinsics {
                assert_gt!(err_k, max_err);
            } else {
                assert_le!(err_k, max_err);
            }
        }
        for (t, tc) in ts.iter().zip(tsc.iter()) {
            let err_t = (tc - t).norm() / tc.norm();
            if optsc.camera_translation {
                assert_gt!(err_t, max_err);
            } else {
                assert_le!(err_t, max_err);
            }
        }
        for (r, rc) in rs.iter().zip(rsc.iter()) {
            let err_r = (rc - r).norm() / rc.norm();
            if optsc.camera_rotation {
                assert_gt!(err_r, max_err);
            } else {
                assert_le!(err_r, max_err);
            }
        }
    }
    */
}
