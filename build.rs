use dotenv::dotenv;
use std::env;

fn build_grpc() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_client(true)
        .compile(&["proto/hub.proto"], &["proto"])?;

    Ok(())
}

fn build_cxx() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let eigen_include_dir = env::var("EIGEN_INCLUDE_DIR").expect("EIGEN_INCLUDE_DIR");
    // println!("cargo:rustc-link-search=/home/oliver/code/rust/eigen-ndarray/cpp");
    // println!("cargo:rustc-link-lib=dylib=stdc++");

    println!("cargo:rerun-if-changed=proto/common.proto");
    println!("cargo:rerun-if-changed=proto/client.proto");
    println!("cargo:rerun-if-changed=proto/server.proto");

    println!("cargo:rerun-if-changed=include/eigen.hpp");
    println!("cargo:rerun-if-changed=src/openmvg/eigen.cpp");

    println!("cargo:rerun-if-changed=include/openmvg.hpp");
    println!("cargo:rerun-if-changed=src/openmvg/openmvg.cpp");

    println!("EIGEN_INCLUDE_DIR = {}", eigen_include_dir);

    cxx_build::bridge("src/openmvg/eigen.rs")
        .file("src/openmvg/eigen.cpp")
        .include(&eigen_include_dir)
        .flag_if_supported("-std=c++14")
        .compile("posenet_vr_eigen");

    cxx_build::bridge("src/openmvg/openmvg.rs")
        .file("src/openmvg/openmvg.cpp")
        .include(&eigen_include_dir)
        .flag_if_supported("-std=c++14")
        .compile("posenet_vr_openmvg");

    // NOTE: `cargo test` fails if this comes before cxx_build::bridge.
    // The error is undefined reference to `openMVG::TriangulateNView(...)'
    // Although strangely, running the same function from a binary works.
    println!("cargo:rustc-link-lib=openMVG_multiview");
    // NOTE: Similarly, numeric must come after multiview
    println!("cargo:rustc-link-lib=openMVG_numeric");

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    build_grpc()?;
    build_cxx()?;

    Ok(())
}
