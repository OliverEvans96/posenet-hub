use std::fmt;

use cxx::memory::UniquePtrTarget;
use cxx::vector::VectorElement;
use cxx::{CxxVector, UniquePtr};
use nalgebra::Vector3;
use nalgebra::{self, Matrix2xX, Matrix3, Matrix3x4, MatrixSlice3xX, OMatrix, Vector2};
use nalgebra::{Dynamic, U2, U3};
use nalgebra::{MatrixSlice3, MatrixSlice3x4, VectorSlice3, VectorSlice4};

pub type Matrix3xN<T> = OMatrix<T, U3, Dynamic>;
pub type Matrix2xN<T> = OMatrix<T, U2, Dynamic>;

#[cxx::bridge]
pub mod ffi {
    // C++ types and signatures exposed to Rust.
    #[namespace = "openMVG"]
    unsafe extern "C++" {
        type Mat2X;
        type Mat3X;
        type Mat34;
        type Mat3;
        type Vec4;
        type Vec3;
        type Vec2;
    }

    unsafe extern "C++" {
        include!("posenet-vr-hub/include/eigen.hpp");

        // Format
        fn format_mat2x(a: &Mat2X) -> UniquePtr<CxxString>;
        fn format_mat3x(a: &Mat3X) -> UniquePtr<CxxString>;
        fn format_mat34(a: &Mat34) -> UniquePtr<CxxString>;
        fn format_mat3(a: &Mat3) -> UniquePtr<CxxString>;
        fn format_vec4(a: &Vec4) -> UniquePtr<CxxString>;
        fn format_vec3(a: &Vec3) -> UniquePtr<CxxString>;
        fn format_vec2(a: &Vec2) -> UniquePtr<CxxString>;

        // To Eigen
        fn mat2x_from_data(slice: &[f64], cols: usize) -> UniquePtr<Mat2X>;
        fn mat3x_from_data(slice: &[f64], cols: usize) -> UniquePtr<Mat3X>;
        fn mat34_from_data(slice: &[f64]) -> UniquePtr<Mat34>;
        fn mat3_from_data(slice: &[f64]) -> UniquePtr<Mat3>;
        fn vec2_from_data(slice: &[f64]) -> UniquePtr<Vec2>;
        fn mat34_vec_from_data(slice: &[&[f64]]) -> UniquePtr<CxxVector<Mat34>>;
        fn mat3_vec_from_data(slice: &[&[f64]]) -> UniquePtr<CxxVector<Mat3>>;
        fn mat2x_vec_from_data(slice: &[&[f64]], cols: usize) -> UniquePtr<CxxVector<Mat2X>>;
        // fn mat3x_vec_from_data(slice: &[&[f64]], cols: usize) -> UniquePtr<CxxVector<Mat3X>>;
        fn vec3_vec_from_data(slice: &[&[f64]]) -> UniquePtr<CxxVector<Vec3>>;

        // To Nalgebra
        fn mat34_to_slice(ptr: &Mat34) -> &[f64];
        fn mat3_to_slice(ptr: &Mat3) -> &[f64];
        fn mat3x_to_slice(ptr: &Mat3X) -> &[f64];
        fn vec4_to_slice(ptr: &Vec4) -> &[f64];
        fn vec3_to_slice(mat: &Vec3) -> &[f64];
    }

    // Explicitly request shim traits since they
    // aren't used in above function signatures
    impl UniquePtr<Mat3X> {}
    impl UniquePtr<Vec4> {}
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

impl ToEigen for Vector2<f64> {
    type T = ffi::Vec2;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let slice = self.as_slice();
        ffi::vec2_from_data(slice)
    }
}

impl ToEigen for Matrix3<f64> {
    type T = ffi::Mat3;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let slice = self.as_slice();
        ffi::mat3_from_data(slice)
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

impl ToEigen for &[Vector3<f64>] {
    type T = CxxVector<ffi::Vec3>;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let slices: Vec<_> = self.iter().map(|mat| mat.as_slice()).collect();
        ffi::vec3_vec_from_data(slices.as_slice())
    }
}

impl ToEigen for &[Matrix3<f64>] {
    type T = CxxVector<ffi::Mat3>;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let slices: Vec<_> = self.iter().map(|mat| mat.as_slice()).collect();
        ffi::mat3_vec_from_data(slices.as_slice())
    }
}

impl ToEigen for &[Matrix2xX<f64>] {
    type T = CxxVector<ffi::Mat2X>;
    fn to_eigen(self) -> UniquePtr<Self::T> {
        let (_rows, cols) = self
            .first()
            .and_then(|mat| Some(mat.shape()))
            .unwrap_or((0, 0));
        let slices: Vec<_> = self.iter().map(|mat| mat.as_slice()).collect();
        ffi::mat2x_vec_from_data(slices.as_slice(), cols)
    }
}

// impl ToNalgebra

pub trait ToNalgebra<'a> {
    type T: 'a;
    fn to_nalgebra(&'a self) -> Self::T;
}

impl<'a> ToNalgebra<'a> for ffi::Mat34 {
    type T = MatrixSlice3x4<'a, f64>;
    fn to_nalgebra(&'a self) -> Self::T {
        let slice = ffi::mat34_to_slice(&self);
        MatrixSlice3x4::from_slice(slice)
    }
}

impl<'a> ToNalgebra<'a> for ffi::Mat3 {
    type T = MatrixSlice3<'a, f64>;
    fn to_nalgebra(&'a self) -> Self::T {
        let slice = ffi::mat3_to_slice(&self);
        MatrixSlice3::from_slice(slice)
    }
}

impl<'a> ToNalgebra<'a> for ffi::Mat3X {
    type T = MatrixSlice3xX<'a, f64>;
    fn to_nalgebra(&'a self) -> Self::T {
        let slice = ffi::mat3x_to_slice(&self);
        let nrows = 3;
        let ncols = slice.len() / nrows;
        MatrixSlice3xX::from_slice(slice, ncols)
    }
}

impl<'a> ToNalgebra<'a> for ffi::Vec4 {
    type T = VectorSlice4<'a, f64>;
    fn to_nalgebra(&'a self) -> Self::T {
        let slice = ffi::vec4_to_slice(&self);
        VectorSlice4::from_slice(slice)
    }
}

impl<'a> ToNalgebra<'a> for ffi::Vec3 {
    type T = VectorSlice3<'a, f64>;
    fn to_nalgebra(&'a self) -> Self::T {
        let slice = ffi::vec3_to_slice(&self);
        VectorSlice3::from_slice(slice)
    }
}

// Blanket impl for vector<I>
impl<'a, I> ToNalgebra<'a> for CxxVector<I>
where
    I: ToNalgebra<'a> + VectorElement,
{
    type T = Vec<I::T>;
    fn to_nalgebra(&'a self) -> Self::T {
        self.iter().map(ToNalgebra::to_nalgebra).collect()
    }
}

// Blanket impl for unique_ptr<I>
impl<'a, I> ToNalgebra<'a> for UniquePtr<I>
where
    I: ToNalgebra<'a> + UniquePtrTarget,
{
    type T = Option<I::T>;
    fn to_nalgebra(&'a self) -> Self::T {
        self.as_ref().map(ToNalgebra::to_nalgebra)
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

impl fmt::Debug for ffi::Mat3 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cxx_str = ffi::format_mat3(self);
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

impl<'a> fmt::Debug for ffi::Vec2 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cxx_str = ffi::format_vec2(self);
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
        let c = b.to_nalgebra().unwrap();
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
    fn test_slice_to_nalgebra() {
        let orig: Vec<_> = (0..3)
            .map(|i| Vector3::new(i as f64, (i as f64) + 0.1, (i as f64) + 0.2))
            .collect();
        println!("orig = {:?}", orig);
        let e = orig.to_eigen();
        println!("e = {:?}", e);
        let n = e.to_nalgebra().unwrap();
        let v: Vec<Vec<_>> = n.iter().map(|vec| vec.iter().collect()).collect();
        println!("v = {:?}", v);
    }
}
