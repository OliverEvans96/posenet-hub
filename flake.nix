{
  inputs = {
    # Fetch git submodules and LFS when this flake is used (Nix 2.27+).
    # Local path (nix build -L .#) uses your working tree; run git submodule update --init proto and git lfs pull.
    # For a clean clone or CI, use: nix build -L 'git+file:///path/to/repo?submodules=1'# (and ensure LFS is enabled).
    self = { submodules = true; lfs = true; };
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    # Optional: proto repo for reference; build uses proto/ from flake source (submodule) when self.submodules = true.
    # proto = { type = "git"; url = "https://gitlab.nrp-nautilus.io/librareome/posenet/posenet-proto.git"; ref = "main"; flake = false; };
    pose-data = {
      type = "git";
      url =
        "https://gitlab.nrp-nautilus.io/librareome/posenet/fake-pose-animation.git";
      ref = "main";
      flake = false;
    };
  };

  outputs = { self, fenix, nixpkgs, flake-utils, crane, pose-data }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        # Deps-only source: only Cargo.toml + Cargo.lock (and .cargo if present).
        # Rebuilds only when dependencies change, not when app source changes.
        depsFilter = path: _type:
          (path == "Cargo.toml" || pkgs.lib.hasSuffix "/Cargo.toml" path)
          || (path == "Cargo.lock" || pkgs.lib.hasSuffix "/Cargo.lock" path)
          || (pkgs.lib.hasInfix ".cargo/" path);
        depsSrc = pkgs.lib.cleanSourceWith {
          src = craneLib.path ./.;
          filter = depsFilter;
          name = "posenet-hub-deps-source";
        };

        # Full package source: everything needed to build the crate (build.rs, proto, include, src).
        packageFilter = path: type:
          (craneLib.filterCargoSources path type)
          || (builtins.match "proto/.*" path != null)
          || (builtins.match "include/.*" path != null)
          || (builtins.match "fake_poses/.*" path != null)
          || (path == "proto" || pkgs.lib.hasSuffix "/proto" path)
          || (path == "fake_poses" || pkgs.lib.hasSuffix "/fake_poses" path)
          || (pkgs.lib.hasInfix "/include/" path)
          || (pkgs.lib.hasInfix "/proto/" path)
          || (pkgs.lib.hasInfix "/fake_poses/" path)
          || (pkgs.lib.hasInfix "/src/" path);
        packageSrc = pkgs.lib.cleanSourceWith {
          src = craneLib.path ./.;
          filter = packageFilter;
          name = "posenet-hub-package-source";
        };

        # Shared args for both stages (env, nativeBuildInputs). Omit src and stage-specific bits.
        commonArgs = {
          nativeBuildInputs = with pkgs; [
            cargo-edit
            rustfmt
            myEigen
            myOpenMVG
            vrpn
            protobuf
          ];
          CARGO_PROFILE = "dev";
          EIGEN_INCLUDE_DIR = "${myEigen}/include/eigen3";
          PROTOC = "${pkgs.protobuf}/bin/protoc";
          RUST_BACKTRACE = "1";
          OMVG = myOpenMVG;
          LIBRARY_PATH = "${myOpenMVG}/lib";
        };
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
        # Stage 1: build only dependencies (dummy app source). Rebuilds when Cargo.toml/Cargo.lock change.
        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
          src = depsSrc;
          pnameSuffix = "-deps";
        });

        # Stage 2: build the package using the dependency artifacts and full source.
        defaultPackage = craneLib.buildPackage (commonArgs // {
          src = packageSrc;
          cargoArtifacts = cargoArtifacts;
        });

        # Pose data as a directory for Docker /data (avoids runAsRoot → no KVM required).
        poseDataDir = pkgs.runCommand "pose-data-dir" { } ''
          mkdir -p $out/data
          cp -r ${pose-data}/* $out/data/
        '';

        packages = {
          dockerImage = pkgs.dockerTools.buildImage {
            name = "posenet-docker";
            tag = "latest";
            # Config options reference:
            # https://github.com/moby/moby/blob/master/image/spec/v1.2.md#image-json-field-descriptions
            config = {
              Env = [ "RUST_LOG=debug" ];
              Cmd = [ "/bin/tini" "-g" "--" "/bin/hub-server" ];
              ExposedPorts = {
                "50051" = { }; # gRPC
                "3883" = { }; # VRPN
              };
            };
            copyToRoot = pkgs.buildEnv {
              name = "image-root";
              paths = with pkgs; [
                tini
                bash
                coreutils
                inetutils
                iana-etc
                netcat-gnu
                defaultPackage
                poseDataDir
              ];
              pathsToLink = [ "/bin" "/etc" "/lib" "/data" ];
            };
          };

          # Minimal image with shell + network utils for debugging (default: hub-server; override with docker run -it ... bash).
          dockerImageDebug = pkgs.dockerTools.buildImage {
            name = "posenet-docker-debug";
            tag = "latest";
            config = {
              Env = [ "RUST_LOG=debug" ];
              Cmd = [ "/bin/tini" "-g" "--" "/bin/hub-server" ];
              ExposedPorts = {
                "50051" = { }; # gRPC
                "3883" = { }; # VRPN
              };
            };
            copyToRoot = pkgs.buildEnv {
              name = "image-root-debug";
              paths = with pkgs; [
                tini
                bash
                coreutils
                inetutils
                iana-etc
                netcat-gnu
                curl
                bind
                iproute2
                defaultPackage
                poseDataDir
              ];
              pathsToLink = [ "/bin" "/etc" "/lib" "/data" ];
            };
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
          OMVG = "${myOpenMVG}";
          EIGEN_INCLUDE_DIR = "${myEigen}/include/eigen3";
          LIBRARY_PATH = "${myOpenMVG}/lib";
          # So "cargo test" can load libstdc++.so.6 (C++ FFI / openMVG)
          LD_LIBRARY_PATH = "${pkgs.stdenv.cc.cc.lib}/lib:${myOpenMVG}/lib";
        };
      });
}
