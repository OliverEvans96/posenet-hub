with import <nixpkgs> {};
let
  myEigen = eigen.overrideAttrs (oldAttrs: rec {
    postInstall = ''
    ln -s $out/include/eigen3/Eigen $out/include/Eigen
    '';
  });
  myOpenMVG =  openmvg.overrideAttrs (oldAttrs: rec {
    # Mimic cpp-deps.Dockerfile
    cmakeFlags = ([
      "-DOpenMVG_BUILD_TYPE=RELEASE"
      "-DOpenMVG_BUILD_SHARED=ON"
      "-DOpenMVG_BUILD_DOC=OFF"
      "-DOpenMVG_BUILD_EXAMPLES=OFF"
      "-DOpenMVG_BUILD_GUI_SOFTWARES=OFF"
      "-DOpenMVG_BUILD_SOFTWARES=OFF"
      "-DOpenMVG_USE_OPENMP=OFF"
      "-DUSE_OPENMP=OFF"
      "-DTARGET_ARCHITECTURE=generic"
    ] ++ oldAttrs.cmakeFlags);
  });
in
stdenv.mkDerivation {
  name = "rust-env";
  nativeBuildInputs = [
    rustc cargo rustfmt

    # Example Build-time Additional Dependencies
    pkg-config
  ];
  buildInputs = [
    myEigen
    myOpenMVG
    vrpn
    protobuf
  ];

  enableParallelBuilding = true;

  buildPhase = ''
    cargo build
  '';

  # Set Environment Variables
  EIGEN_INCLUDE_DIR="${myEigen}/include/eigen3";
  PROTOC="${protobuf}/bin/protoc";
  RUST_BACKTRACE = 1;
  OMVG=myOpenMVG;
}

