use std::fmt;

use cxx::{UniquePtr, UniquePtrTarget};
use nalgebra::{self, Matrix3x4, MatrixMN};

#[cxx::bridge]
mod ffi {
    // C++ types and signatures exposed to Rust.
    #[namespace = "openMVG"]
    unsafe extern "C++" {
        type Mat3X;
        type Mat34;
    }

    unsafe extern "C++" {
        include!("posenet-vr-hub/include/openmvg.hpp");

        fn format_mat3x(a: &Mat3X) -> UniquePtr<CxxString>;
        fn format_mat34(a: &Mat34) -> UniquePtr<CxxString>;
        fn mat3x_from_data(slice: &mut [f64], rows: usize, cols: usize) -> UniquePtr<Mat3X>;
        fn mat34_from_data(slice: &mut [f64], rows: usize, cols: usize) -> UniquePtr<Mat34>;
    }
}
trait EigenMat {}
impl EigenMat for ffi::Mat3X {}
impl EigenMat for ffi::Mat34 {}

type Matrix3xN<T> = MatrixMN<T, nalgebra::U3, nalgebra::Dynamic>;

trait ToEigen {
    type T: EigenMat + UniquePtrTarget;
    fn to_eigen(self) -> UniquePtr<Self::T>;
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
}
