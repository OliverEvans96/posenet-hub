use core::f64;
use std::fmt;
use std::ops::Mul;

use cxx::{CxxVector, UniquePtr, UniquePtrTarget};
use generic_array::ArrayLength;
use nalgebra::{self, Matrix3x4, MatrixMN};
use nalgebra::{DimName, Dynamic, U1, U2, U3, U4};
use nalgebra::{MatrixSlice3x4, MatrixSliceMN, VectorSlice4};
use nalgebra::{Point2, Point3, Rotation3, Vector3};

type Matrix3xN<T> = MatrixMN<T, U3, Dynamic>;
type Matrix2xN<T> = MatrixMN<T, U2, Dynamic>;

#[cxx::bridge]
mod ffi {
    #[derive(Debug)]
    struct NViewPartialDataset {
        /// 3D points.
        x3d: UniquePtr<Mat3X>,
        /// Projected points; may have noise added.
        x2d_vec: UniquePtr<CxxVector<Mat2X>>,
        /// Actual number of cameras.
        n: usize,
        ///-- Return P=K*[R|t] for the Inth camera
        p_vec: UniquePtr<CxxVector<Mat34>>,
    }

    // C++ types and signatures exposed to Rust.
    #[namespace = "openMVG"]
    unsafe extern "C++" {
        type Mat2X;
        type Mat3X;
        type Mat34;
        type Vec3;
        type Vec4;
    }

    unsafe extern "C++" {
        include!("posenet-vr-hub/include/openmvg.hpp");

        // Format
        fn format_mat2x(a: &Mat2X) -> UniquePtr<CxxString>;
        fn format_mat3x(a: &Mat3X) -> UniquePtr<CxxString>;
        fn format_mat34(a: &Mat34) -> UniquePtr<CxxString>;
        fn format_vec3(a: &Vec3) -> UniquePtr<CxxString>;
        fn format_vec4(a: &Vec4) -> UniquePtr<CxxString>;

        // To Eigen
        fn mat2x_from_data(slice: &[f64], cols: usize) -> UniquePtr<Mat2X>;
        fn mat3x_from_data(slice: &[f64], cols: usize) -> UniquePtr<Mat3X>;
        fn mat34_from_data(slice: &[f64]) -> UniquePtr<Mat34>;
        fn mat34_vec_from_data(slice: &[&[f64]]) -> UniquePtr<CxxVector<Mat34>>;

        // To Nalgebra
        fn mat34_to_slice(slice: &UniquePtr<Mat34>) -> &[f64];
        fn vec4_to_slice(slice: &UniquePtr<Vec4>) -> &[f64];

        /// x's are landmark bearing vectors in each camera
        /// Ps are projective cameras
        fn triangulate_nview(
            x: UniquePtr<Mat3X>,
            Ps: UniquePtr<CxxVector<Mat34>>,
        ) -> UniquePtr<Vec4>;
    }
}

// impl ToEigen

trait ToEigen {
    type T: UniquePtrTarget;
    fn to_eigen(self) -> UniquePtr<Self::T>;
}

impl ToEigen for Matrix2xN<f64> {
    type T = ffi::Mat2X;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let (_rows, cols) = self.shape();
        let slice = self.as_slice();
        ffi::mat2x_from_data(slice, cols)
    }
}

impl ToEigen for Matrix3xN<f64> {
    type T = ffi::Mat3X;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let (_rows, cols) = self.shape();
        let slice = self.as_slice();
        ffi::mat3x_from_data(slice, cols)
    }
}

impl ToEigen for Matrix3x4<f64> {
    type T = ffi::Mat34;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let slice = self.as_slice();
        ffi::mat34_from_data(slice)
    }
}

impl ToEigen for &[Matrix3x4<f64>] {
    type T = CxxVector<ffi::Mat34>;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let slices: Vec<_> = self.iter().map(|mat| mat.as_slice()).collect();
        ffi::mat34_vec_from_data(slices.as_slice())
    }
}

// impl ToNalgebra

trait ToNalgebra<M, N>
where
    M: DimName,
    N: DimName,
    M::Value: Mul<N::Value>,
    <M::Value as Mul<N::Value>>::Output: ArrayLength<f64>,
{
    fn to_nalgebra<'a>(&'a self) -> MatrixSliceMN<'a, f64, M, N>;
}

impl ToNalgebra<U3, U4> for UniquePtr<ffi::Mat34> {
    fn to_nalgebra<'a>(&'a self) -> MatrixSlice3x4<'a, f64> {
        let slice = ffi::mat34_to_slice(&self);
        MatrixSlice3x4::from_slice(slice)
    }
}

impl ToNalgebra<U4, U1> for UniquePtr<ffi::Vec4> {
    fn to_nalgebra<'a>(&'a self) -> VectorSlice4<'a, f64> {
        let slice = ffi::vec4_to_slice(&self);
        VectorSlice4::from_slice(slice)
    }
}

// impl Debug

impl fmt::Debug for ffi::Mat34 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cxx_str = ffi::format_mat34(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}

impl fmt::Debug for ffi::Mat2X {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cxx_str = ffi::format_mat2x(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}

impl fmt::Debug for ffi::Mat3X {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cxx_str = ffi::format_mat3x(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}

impl fmt::Debug for ffi::Vec3 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cxx_str = ffi::format_vec3(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}

impl fmt::Debug for ffi::Vec4 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cxx_str = ffi::format_vec4(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}

pub fn triangulate(
    points2d: &[Point2<f64>],
    camera_poses: &[Matrix3x4<f64>],
) -> UniquePtr<ffi::Vec4> {
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

    let x3d = ffi::triangulate_nview(x2d_h_mat, camera_mat);

    // TODO: Convert back to nalgebra point
    return x3d;
}

pub fn triangulate_many(
    points2d_slice: &[&[Point2<f64>]],
    camera_poses: &mut [Matrix3x4<f64>],
) -> Vec<UniquePtr<ffi::Vec4>> {
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
    use nalgebra::{Point2, Point3, Vector2, Vector4};

    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn nalgebra_to_eigen() {
        let m = Matrix3x4::<f64>::new_random();
        let p = m.to_eigen();
        println!("A = \n{:?}", p);
    }

    #[test]
    fn two_way_matrix_conversion() {
        let a = Matrix3x4::<f64>::new_random();
        let b = a.to_eigen();
        let c = b.to_nalgebra();
        println!("a = {}", a);
        println!("b = {:?}", b);
        println!("c = {}", c);
    }

    #[test]
    fn create_mat34_vec_and_print() {
        let n = 4;
        let mut mats: Vec<Matrix3x4<f64>> =
            (0..n).map(|_| Matrix3x4::<f64>::new_random()).collect();
        // let mats: &mut [Matrix3x4<f64>];
        let mut e_mats = mats.as_mut_slice().to_eigen();
        let pin = e_mats.pin_mut();
        for i in 0..n {
            let e_mat = pin
                .get(i)
                .expect(format!("Could not get CxxVector element {}", i).as_str());
            println!("i = {}\n{:?}\n", i, e_mat)
        }
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

        let x3d: UniquePtr<ffi::Vec4> =
            triangulate(points2d.as_slice(), camera_poses.as_mut_slice());
        println!("RAND x3d = {:?}", x3d);
    }

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
            let x3d_recon_h_eig = triangulate(x2d_vec.as_slice(), &mut p_vec);
            println!("About to convert");
            // TODO: Avoid copying? Does this copy?
            let x3d_recon_h: Vector4<f64> = x3d_recon_h_eig.to_nalgebra().into();
            let x3d_recon = Point3::<f64>::from_homogeneous(x3d_recon_h)
                .expect("Reconstructed 3D point was not homogeneous");
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
