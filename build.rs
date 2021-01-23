use dotenv::dotenv;
use std::{env, error};

type UnitResult = Result<(), Box<dyn error::Error>>;

fn build_grpc() -> UnitResult {
    // gRPC
    println!("cargo:rerun-if-changed=proto/common.proto");
    println!("cargo:rerun-if-changed=proto/client.proto");
    println!("cargo:rerun-if-changed=proto/server.proto");

    tonic_build::configure()
        .build_client(true)
        .compile(&["proto/hub.proto"], &["proto"])?;

    Ok(())
}

fn build_cxx() -> UnitResult {
    dotenv().ok();
    let eigen_include_dir = env::var("EIGEN_INCLUDE_DIR").expect("EIGEN_INCLUDE_DIR");

    println!("cargo:rerun-if-changed=include/eigen.hpp");
    println!("cargo:rerun-if-changed=src/openmvg/eigen.cpp");
    println!("cargo:rerun-if-changed=include/openmvg.hpp");
    println!("cargo:rerun-if-changed=src/openmvg/openmvg.cpp");

    cxx_build::bridge("src/openmvg/eigen.rs")
        .file("src/openmvg/eigen.cpp")
        .include(&eigen_include_dir)
        .flag_if_supported("-std=c++14")
        // Without this flag, I was getting random segfaults.
        // See https://github.com/openMVG/openMVG/issues/1847
        .flag_if_supported("-march=native")
        .compile("posenet_vr_eigen");

    cxx_build::bridge("src/openmvg/openmvg.rs")
        .file("src/openmvg/openmvg.cpp")
        .include(&eigen_include_dir)
        .flag_if_supported("-std=c++14")
        // Without this flag, I was getting random segfaults.
        // See https://github.com/openMVG/openMVG/issues/1847
        .flag_if_supported("-march=native")
        .compile("posenet_vr_openmvg");

    // NOTE: `cargo test` fails if this comes before cxx_build::bridge.
    // The error is undefined reference to `openMVG::TriangulateNView(...)'
    // Although strangely, running the same function from a binary works.
    println!("cargo:rustc-link-lib=openMVG_multiview");
    // NOTE: Similarly, numeric must come after multiview
    println!("cargo:rustc-link-lib=openMVG_numeric");

    Ok(())
}

fn build_vrpn() -> UnitResult {
    dotenv().ok();

    println!("cargo:rerun-if-changed=include/vrpn.hpp");
    println!("cargo:rerun-if-changed=src/openmvg/vrpn.cpp");

    cxx_build::bridge("src/vrpn/vrpn.rs")
        .file("src/vrpn/vrpn.cpp")
        .flag_if_supported("-std=c++14")
        // Without this flag, I was getting random segfaults.
        // See https://github.com/openMVG/openMVG/issues/1847
        .flag_if_supported("-march=native")
        .compile("posenet_vr_vrpn");

    println!("cargo:rustc-link-lib=vrpn");
    // TODO: Might not need this after disabling tracker
    println!("cargo:rustc-link-lib=quat");

    Ok(())
}

fn main() -> UnitResult {
    build_grpc()?;
    build_cxx()?;
    build_vrpn()?;

    Ok(())
}
