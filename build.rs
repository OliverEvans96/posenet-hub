use dotenv::dotenv;
use std::{env, error};

type UnitResult = Result<(), Box<dyn error::Error>>;

fn build_grpc() -> UnitResult {
    // gRPC
    println!("cargo:rerun-if-changed=proto/common.proto");
    println!("cargo:rerun-if-changed=proto/client.proto");
    println!("cargo:rerun-if-changed=proto/server.proto");

    dotenv().ok();

    let mut proto_includes = vec!["proto"];
    let extra_proto_inc_str = env::var("EXTRA_PROTO_INC").unwrap_or_default();
    proto_includes.extend(extra_proto_inc_str.split(":"));

    tonic_build::configure()
        .build_client(true)
        // Allow auto-generated tonic / gRPC types to be
        // serialized and deserialized with serde
        // See:
        // 1. https://www.reddit.com/r/rust/comments/efuikd/comment/fc3d6c6/?utm_source=share&utm_medium=web2x&context=3
        // 2. https://docs.rs/prost-build/0.8.0/prost_build/struct.Config.html#method.type_attribute
        // 3. https://docs.rs/prost-build/0.8.0/prost_build/struct.Config.html#method.btree_map
        // 4. https://www.reddit.com/r/rust/comments/efuikd/comment/fc3r7ty/?utm_source=share&utm_medium=web2x&context=3
        .type_attribute("Point2D", "#[derive(serde::Deserialize, serde::Serialize)]")
        .type_attribute("Point3D", "#[derive(serde::Deserialize, serde::Serialize)]")
        .type_attribute("Pose2D", "#[derive(serde::Deserialize, serde::Serialize)]")
        .type_attribute("Pose3D", "#[derive(serde::Deserialize, serde::Serialize)]")
        .type_attribute(
            "CameraIntrinsics",
            "#[derive(serde::Deserialize, serde::Serialize)]",
        )
        .type_attribute(
            "CameraExtrinsics",
            "#[derive(serde::Deserialize, serde::Serialize)]",
        )
        // Allow tokens and camera identifier to be used as HashMap keys
        .type_attribute("CommandToken", "#[derive(Hash,Eq)]")
        .type_attribute("SessionToken", "#[derive(Hash,Eq)]")
        .type_attribute("CameraIdentifier", "#[derive(Hash,Eq)]")
        .compile(&["hub.proto"], &proto_includes)?;

    Ok(())
}

fn build_cxx() -> UnitResult {
    dotenv().ok();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let eigen_include_dir =
        env::var("EIGEN_INCLUDE_DIR").unwrap_or("/usr/include/eigen3".to_owned());

    println!("cargo:rerun-if-changed=include/eigen.hpp");
    println!("cargo:rerun-if-changed=src/openmvg/eigen.cpp");
    println!("cargo:rerun-if-changed=include/openmvg.hpp");
    println!("cargo:rerun-if-changed=src/openmvg/openmvg.cpp");

    cxx_build::bridge("src/openmvg/eigen.rs")
        .file("src/openmvg/eigen.cpp")
        .include(&manifest_dir)
        .include(&eigen_include_dir)
        .flag_if_supported("-std=c++17")
        // Building for the wrong architecture can cause segfaults
        // See https://github.com/openMVG/openMVG/issues/1847
        .flag_if_supported("-mtune=generic")
        .compile("posenet_vr_eigen");

    cxx_build::bridge("src/openmvg/openmvg.rs")
        .file("src/openmvg/openmvg.cpp")
        .include(&manifest_dir)
        .include(&eigen_include_dir)
        .flag_if_supported("-std=c++17")
        // Building for the wrong architecture can cause segfaults
        // See https://github.com/openMVG/openMVG/issues/1847
        .flag_if_supported("-mtune=generic")
        .compile("posenet_vr_openmvg");

    // NOTE: `cargo test` fails if this comes before cxx_build::bridge.
    // The error is undefined reference to `openMVG::TriangulateNView(...)'
    // Although strangely, running the same function from a binary works.
    if let Ok(omvg_path) = env::var("OMVG") {
        println!("cargo:rustc-link-search=native={}/lib", omvg_path);
    }
    println!("cargo:rustc-link-lib=openMVG_sfm");
    println!("cargo:rustc-link-lib=openMVG_ceres");
    println!("cargo:rustc-link-lib=openMVG_cxsparse");
    println!("cargo:rustc-link-lib=openMVG_multiview");
    println!("cargo:rustc-link-lib=openMVG_geometry");
    // NOTE: Similarly, numeric must come after multiview
    println!("cargo:rustc-link-lib=openMVG_numeric");

    Ok(())
}

fn build_vrpn() -> UnitResult {
    dotenv().ok();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let is_debug = env::var("PROFILE").unwrap_or_default() == "debug";

    println!("cargo:rerun-if-changed=include/vrpn.hpp");
    println!("cargo:rerun-if-changed=src/openmvg/vrpn.cpp");

    let mut vrpn_build = cxx_build::bridge("src/vrpn/vrpn.rs");
    vrpn_build
        .file("src/vrpn/vrpn.cpp")
        .include(&manifest_dir)
        .flag_if_supported("-std=c++14")
        // Building for the wrong architecture can cause segfaults
        // See https://github.com/openMVG/openMVG/issues/1847
        .flag_if_supported("-mtune=generic");
    if is_debug {
        vrpn_build.flag("-U_FORTIFY_SOURCE");
    }
    vrpn_build.compile("posenet_vr_vrpn");

    println!("cargo:rustc-link-lib=vrpn");

    Ok(())
}

fn main() -> UnitResult {
    build_grpc()?;
    build_cxx()?;
    build_vrpn()?;

    Ok(())
}
