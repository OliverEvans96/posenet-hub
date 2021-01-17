use dotenv::dotenv;
use std::env;

fn main() {
    dotenv().ok();
    let eigen_include_dir = env::var("EIGEN_INCLUDE_DIR").expect("EIGEN_INCLUDE_DIR");
    // println!("cargo:rustc-link-search=/home/oliver/code/rust/eigen-ndarray/cpp");
    // println!("cargo:rustc-link-lib=eigen_ndarray");
    // println!("cargo:rustc-link-lib=dylib=stdc++");

    println!("cargo:rerun-if-changed=include/openmvg.hpp");
    println!("cargo:rerun-if-changed=src/openmvg.cpp");

    println!("EIGEN_INCLUDE_DIR = {}", eigen_include_dir);

    cxx_build::bridge("src/lib.rs")
        .file("src/openmvg.cpp")
        .include(eigen_include_dir)
        .flag_if_supported("-std=c++14")
        .compile("posenet-vr-hub");
}
