use std::fmt;
use std::ops::Mul;

use generic_array::ArrayLength;

use cxx::{CxxVector, UniquePtr, UniquePtrTarget};
use nalgebra::DimName;
use nalgebra::{self, Matrix3x4, MatrixMN};
use nalgebra::{Dynamic, U1, U2, U3, U4};
use nalgebra::{MatrixSlice3x4, MatrixSliceMN, VectorSlice4};

pub type Matrix3xN<T> = MatrixMN<T, U3, Dynamic>;
pub type Matrix2xN<T> = MatrixMN<T, U2, Dynamic>;

#[cxx::bridge]
pub mod ffi {
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
        include!("posenet-vr-hub/include/eigen.hpp");

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
    }
}

// impl ToEigen

pub trait ToEigen {
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

pub trait ToNalgebra<M, N>
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
