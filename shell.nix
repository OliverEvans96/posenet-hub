with import <nixpkgs> {};
let
  myEigen = eigen.overrideAttrs (oldAttrs: rec {
    postInstall = ''
    ln -s $out/include/eigen3/Eigen $out/include/Eigen
    '';
  });
  myOpenMVG =  openmvg.overrideAttrs (oldAttrs: rec {
    cmakeFlags = (
      [ "-DOpenMVG_BUILD_SHARED=ON" ]
      ++ oldAttrs.cmakeFlags
    );
  });
in
stdenv.mkDerivation {
  name = "rust-env";
  nativeBuildInputs = [
    rustc cargo

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

