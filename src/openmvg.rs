#[cxx::bridge]
mod ffi {
    // C++ types and signatures exposed to Rust.
    #[namespace = "OpenMVG"]
    unsafe extern "C++" {
        type Mat3X;
        type Mat34;
    }

    unsafe extern "C++" {
        include!("eigen-ndarray/include/eigen_ndarray.hpp");

        fn format_matxf(a: &MatrixXf) -> UniquePtr<CxxString>;
        fn format_mat3f(a: &Matrix3f) -> UniquePtr<CxxString>;
        fn create_eye() -> UniquePtr<Matrix3f>;
        fn create_another() -> UniquePtr<Matrix3f>;
        fn mult3f(a: &Matrix3f, b: &Matrix3f) -> UniquePtr<Matrix3f>;
        fn multxf(a: &MatrixXf, b: &MatrixXf) -> UniquePtr<MatrixXf>;

        fn eigen_from_data(slice: &mut [f32], rows: usize, cols: usize) -> UniquePtr<MatrixXf>;
    }
}

impl fmt::Debug for ffi::Matrix3f {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cxx_str = ffi::format_mat3f(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}

impl fmt::Debug for ffi::MatrixXf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cxx_str = ffi::format_matxf(self);
        let s = cxx_str
            .as_ref()
            .expect("Pointer had no value.")
            .to_str()
            .expect("Could not convert string");

        f.write_str(s)
    }
}
