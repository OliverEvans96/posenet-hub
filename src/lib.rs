use std::fmt;

use cxx::{UniquePtr, UniquePtrTarget};
use nalgebra::{self, Matrix3x4, MatrixMN};

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
        fn mat2x_from_data(slice: &mut [f64], rows: usize, cols: usize) -> UniquePtr<Mat2X>;
        fn mat3x_from_data(slice: &mut [f64], rows: usize, cols: usize) -> UniquePtr<Mat3X>;
        fn mat34_from_data(slice: &mut [f64], rows: usize, cols: usize) -> UniquePtr<Mat34>;

        fn create_nview_dataset(nview: i32, npoints: i32) -> NViewPartialDataset;

        /// x's are landmark bearing vectors in each camera
        /// Ps are projective cameras
        fn triangulate_nview(x: &Mat3X, Ps: &CxxVector<Mat34>, X: UniquePtr<Vec4>);
    }
}
trait EigenMat {}
impl EigenMat for ffi::Mat2X {}
impl EigenMat for ffi::Mat3X {}
impl EigenMat for ffi::Mat34 {}
impl EigenMat for ffi::Vec4 {}

type Matrix3xN<T> = MatrixMN<T, nalgebra::U3, nalgebra::Dynamic>;
type Matrix2xN<T> = MatrixMN<T, nalgebra::U2, nalgebra::Dynamic>;

trait ToEigen {
    type T: EigenMat + UniquePtrTarget;
    fn to_eigen(self) -> UniquePtr<Self::T>;
}

impl ToEigen for Matrix2xN<f64> {
    type T = ffi::Mat2X;
    fn to_eigen(mut self) -> UniquePtr<Self::T> {
        let (rows, cols) = self.shape();
        let slice = self.as_mut_slice();
        ffi::mat2x_from_data(slice, rows, cols)
    }
}

impl ToEigen for Matrix3xN<f64> {
    type T = ffi::Mat3X;
    fn to_eigen(mut self) -> UniquePtr<Self::T> {
        let (rows, cols) = self.shape();
        let slice = self.as_mut_slice();
        ffi::mat3x_from_data(slice, rows, cols)
    }
}

impl ToEigen for Matrix3x4<f64> {
    type T = ffi::Mat34;
    fn to_eigen(mut self) -> UniquePtr<Self::T> {
        let (rows, cols) = self.shape();
        let slice = self.as_mut_slice();
        ffi::mat34_from_data(slice, rows, cols)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_create_nview_dataset() {
        let d = ffi::create_nview_dataset(3, 4);
        println!("d = {:?}", d);
    }

    #[test]
    fn test_triangulate() {
        let m = Matrix3x4::<f64>::new(1.0, 3.5, 1.2, 6.2, 1.2, 3.5, 3.6, 7.8, 8.3, 2.1, 1.7, 9.8);
        let p = m.to_eigen();
        println!("A = \n{:?}", p);
    }
}
