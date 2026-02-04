{
  inputs = {
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    proto = {
      type = "git";
      url =
        "https://gitlab.nrp-nautilus.io/librareome/posenet/posenet-proto.git";
      ref = "next";
      flake = false;
    };
    # "git+ssh://git@gitlab-ssh.nrp-nautilus.io:30622/librareome/posenet/posenet-proto.git/main";
    # "https://gitlab.nrp-nautilus.io/librareome/posenet/posenet-proto.git/main";
    # "git+ssh://git@gitlab-ssh.nrp-nautilus.io:30622/librareome/posenet/posenet-proto.git/main";
    pose-data = {
      type = "git";
      url =
        "https://gitlab.nrp-nautilus.io/librareome/posenet/fake-pose-animation.git";
      ref = "main";
      flake = false;
    };
  };

  outputs = { self, fenix, nixpkgs, flake-utils, crane, proto, pose-data }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        myEigen = pkgs.eigen.overrideAttrs (oldAttrs: rec {
          postInstall = ''
            ln -s $out/include/eigen3/Eigen $out/include/Eigen
          '';
        });
        # openMVG 1.6 (matches build/cpp-deps.Dockerfile): builds internal
        # libopenMVG_ceres and libopenMVG_cxsparse required by our build.rs
        openmvg_1_6_src = pkgs.fetchFromGitHub {
          owner = "openMVG";
          repo = "openMVG";
          rev = "v1.6";
          sha256 = "sha256-MDQeRPa6p4qQ7+jciCBRiSFm89k95RHsl+zE9xuIOlc=";
          fetchSubmodules = true;
        };
        myOpenMVG = pkgs.stdenv.mkDerivation {
          pname = "openmvg";
          version = "1.6";
          src = openmvg_1_6_src;
          nativeBuildInputs = [ pkgs.cmake ];
          buildInputs = [ myEigen ];
          # Vendored CoinUtils triggers -Werror=format-security with modern GCC
          hardeningDisable = [ "format" ];
          # Out-of-tree build like cpp-deps.Dockerfile: cmake ../openMVG/src from build dir
          preConfigure = "mkdir -p build && cd build";
          configurePhase = ''
            runHook preConfigure
            cmake -DCMAKE_INSTALL_PREFIX=$out \
              -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
              -DCMAKE_BUILD_TYPE=RELEASE \
              -DOpenMVG_BUILD_DOC=OFF \
              -DOpenMVG_BUILD_EXAMPLES=OFF \
              -DOpenMVG_BUILD_GUI_SOFTWARES=OFF \
              -DOpenMVG_BUILD_SOFTWARES=OFF \
              -DOpenMVG_USE_OPENMP=OFF \
              -DUSE_OPENMP=OFF \
              -DOPENMP=OFF \
              -DTARGET_ARCHITECTURE=generic \
              ../src
          '';
          # Nix runs each phase in a fresh shell; build dir is $NIX_BUILD_TOP/source/build
          buildPhase = "cd $NIX_BUILD_TOP/source/build && make -j$NIX_BUILD_CORES";
          installPhase = "cd $NIX_BUILD_TOP/source/build && make install";
        };
      in rec {
        defaultPackage = crane.lib.${system}.buildPackage {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            cargo-edit
            rustfmt

            myEigen
            myOpenMVG
            pkgs.vrpn
            pkgs.protobuf
          ];

          src = ./.;

          # enableParallelBuilding = true;

          # Set Environment Variables
          EXTRA_PROTO_INC = proto;
          EIGEN_INCLUDE_DIR = "${myEigen}/include/eigen3";
          PROTOC = "${pkgs.protobuf}/bin/protoc";
          RUST_BACKTRACE = 1;
          OMVG = myOpenMVG;
        };

        packages = {
          dockerImage = pkgs.dockerTools.buildImage {
            name = "posenet-docker";
            tag = "latest";
            # Config options reference:
            # https://github.com/moby/moby/blob/master/image/spec/v1.2.md#image-json-field-descriptions
            config = {
              Cmd = [ "${defaultPackage}/bin/hub-server" ];
              ExposedPorts = {
                "50051" = { }; # gRPC
                "3883" = { }; # VRPN
              };
            };
            contents = with pkgs; [
              bash # bash
              coreutils # ls, cat, etc
              inetutils # ip, ifconfig, etc.
              iana-etc # /etc/protocols
              netcat-gnu # nc
              defaultPackage # posenet-hub
              pose-data.defaultPackage.${system} # /data/poses.json
            ];
          };
          testPackage = pkgs.stdenv.mkDerivation {
            name = "testPackage";
            buildInputs = with pkgs; [ curl ];
            src = ./.;
            buildPhase = ''
              curl https://github.com
            '';
          };
        };
        devShell = pkgs.mkShell {
          name = "rust-env";
          src = ./.;

          # build-time deps
          # from https://blog.thomasheartman.com/posts/bevy-getting-started-on-nixos
          nativeBuildInputs = with pkgs; [
            # rustc
            # cargo
            # rustfmt

            lld
            clang

            cargo-edit
            cargo-watch

            grpc-tools
            myEigen
            myOpenMVG
            vrpn
          ];

          # In order to avoid the following error:
          # "failed to invoke protoc
          # (hint: https://docs.rs/prost-build/#sourcing-protoc):
          # No such file or directory (os error 2)"
          PROTOC = "${pkgs.grpc-tools}/bin/protoc";
          # FIXME (find a better solution - this only works on my laptop)
          # PROTOC_INCLUDE =
          #   "/home/oliver/ucsd/posenet-vr/hub/proto:${pkgs.protobuf}/include";

          # FIXME (without this env var)
          # CPLUS_INCLUDE_PATH = "${myEigen}/include/eigen3";
          OMVG = "${myOpenMVG}";
          EIGEN_INCLUDE_DIR = "${myEigen}/include/eigen3";
          LIBRARY_PATH = "${myOpenMVG}/lib";
          # So "cargo test" can load libstdc++.so.6 (C++ FFI / openMVG)
          LD_LIBRARY_PATH = "${pkgs.stdenv.cc.cc.lib}/lib:${myOpenMVG}/lib";
        };
      });
}
