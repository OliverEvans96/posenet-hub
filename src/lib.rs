use std::fmt;

use cxx::{CxxVector, UniquePtr, UniquePtrTarget};
use nalgebra::{self, Matrix3x4, MatrixMN};
// use nalgebra::{Point2, Point3, Vector3};

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
        type Vec4;
    }

    unsafe extern "C++" {
        include!("posenet-vr-hub/include/openmvg.hpp");

        fn format_mat2x(a: &Mat2X) -> UniquePtr<CxxString>;
        fn format_mat3x(a: &Mat3X) -> UniquePtr<CxxString>;
        fn format_mat34(a: &Mat34) -> UniquePtr<CxxString>;
        fn format_vec4(a: &Vec4) -> UniquePtr<CxxString>;
        fn mat2x_from_data(slice: &mut [f64], cols: usize) -> UniquePtr<Mat2X>;
        fn mat3x_from_data(slice: &mut [f64], cols: usize) -> UniquePtr<Mat3X>;
        fn mat34_from_data(slice: &mut [f64]) -> UniquePtr<Mat34>;

        fn mat34_vec_from_data(slice: &[&mut [f64]]) -> UniquePtr<CxxVector<Mat34>>;

    // fn print_mat34_vec(v: UniquePtr<CxxVector<Mat34>>);

    // fn create_nview_dataset(nview: i32, npoints: i32) -> NViewPartialDataset;

    // x's are landmark bearing vectors in each camera
    // Ps are projective cameras
    // fn triangulate_nview(x: &Mat3X, Ps: &CxxVector<Mat34>, X: UniquePtr<Vec4>);
    }
}

// impl ToEigen

type Matrix3xN<T> = MatrixMN<T, nalgebra::U3, nalgebra::Dynamic>;
type Matrix2xN<T> = MatrixMN<T, nalgebra::U2, nalgebra::Dynamic>;

trait ToEigen {
    type T: UniquePtrTarget;
    fn to_eigen(self) -> UniquePtr<Self::T>;
}

impl ToEigen for Matrix2xN<f64> {
    type T = ffi::Mat2X;
    fn to_eigen(mut self) -> UniquePtr<Self::T> {
        let (_rows, cols) = self.shape();
        let slice = self.as_mut_slice();
        ffi::mat2x_from_data(slice, cols)
    }
}

impl ToEigen for Matrix3xN<f64> {
    type T = ffi::Mat3X;
    fn to_eigen(mut self) -> UniquePtr<Self::T> {
        let (_rows, cols) = self.shape();
        let slice = self.as_mut_slice();
        ffi::mat3x_from_data(slice, cols)
    }
}

impl ToEigen for Matrix3x4<f64> {
    type T = ffi::Mat34;
    fn to_eigen(mut self) -> UniquePtr<Self::T> {
        let slice = self.as_mut_slice();
        ffi::mat34_from_data(slice)
    }
}

impl ToEigen for &mut [Matrix3x4<f64>] {
    type T = CxxVector<ffi::Mat34>;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let mut slices = Vec::<&mut [f64]>::with_capacity(self.len());
        for mat in self.iter_mut() {
            let slice = mat.as_mut_slice();
            slices.push(slice);
        }
        ffi::mat34_vec_from_data(slices.as_mut_slice())
    }
}

// impl Debug

impl fmt::Debug for ffi::Mat34 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cxx_str = ffi::format_mat3x(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}

impl fmt::Debug for ffi::Vec4 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cxx_str = ffi::format_vec4(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}

/*
fn triangulate(points2d: &[Point2<f64>], camera_poses: &[Matrix3x4<f64>]) -> Point3<f64> {
    assert_eq!(points2d.len(), camera_poses.len());
    let x2d_h_mat = Matrix3xN::<f64>::from_columns(
        points2d
            .iter()
            .map(|x2d| x2d.to_homogeneous())
            .collect::<Vec<Vector3<f64>>>()
            .as_slice(),
    )
    .to_eigen();
}
*/

/*
fn triangulate_many<T: nalgebra::Scalar>(
    points2d_slice: &[&[Point2<T>]],
    camera_poses: &[Matrix3x4<T>],
) -> Vec<Point3<T>> {
    let points3d: Vec<Point3<T>>;
    points2d
        .iter()
        .map(|x2d| triangulate_one(x2d, camera_poses))
        .collect()
}
*/

#[cfg(test)]
mod tests {
    use super::*;
    // use nalgebra::{Point2, Point3, Vector3};

    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn nalgebra_to_eigen() {
        let m = Matrix3x4::<f64>::new(1.0, 3.5, 1.2, 6.2, 1.2, 3.5, 3.6, 7.8, 8.3, 2.1, 1.7, 9.8);
        let p = m.to_eigen();
        println!("A = \n{:?}", p);
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

    /*
    #[test]
    fn test_create_nview_dataset() {
        let d = ffi::create_nview_dataset(3, 4);
        println!("d = {:?}", d);
    }
    */

    /*
    #[test]
    fn test_triangulate() {
        // let d = ffi::create_nview_dataset(3, 4);
        let nviews = 3;
        let npoints = 7;

        // Create 3D points and cameras
        let x3d_vec: Vec<Vector3<f64>> = (0..npoints).map(|_| Vector3::new_random()).collect();
        let p_vec: Vec<Matrix3x4<f64>> = (0..nviews).map(|_| Matrix3x4::new_random()).collect();

        // Combine 3D points into a matrix
        let x3d_mat = Matrix3xN::<f64>::from_columns(x3d_vec.as_slice());

        // Create homogeneous projections for each camera
        let x2d_h_vec: Vec<Vector3<f64>> = x3d_vec
            .iter()
            .zip(p_vec.iter())
            .map(|(x3d, p)| p * Point3::<f64>::from(*x3d).to_homogeneous())
            .collect();

        let x2d_vec: Vec<Point2<f64>> = x2d_h_vec
            .iter()
            .map(|&x2d_h| {
                Point2::<f64>::from_homogeneous(x2d_h)
                    .expect("Vector was apparently not homogeneous")
            })
            .collect();

        println!("x3d_mat = {}", x3d_mat);

        for i in 0..nviews {
            println!("p[{}] = {}", i, p_vec[i]);
            println!("x2d[{}] = {}", i, x2d_vec[i]);
        }

        let x3d_t = triangulate(x2d_vec, p_vec);
    }
    */
}
