{
  inputs = {
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "nixpkgs/nixos-unstable";
    # nixpkgs.url = "github:OliverEvans96/nixpkgs/bump-rust-analyzer-2022-05-02";
    flake-utils.url = "github:numtide/flake-utils";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    proto = {
      type = "git";
      url =
        "https://gitlab.nrp-nautilus.io/librareome/posenet/posenet-proto.git";
      ref = "main";
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
        myOpenMVG = pkgs.openmvg.overrideAttrs (oldAttrs: rec {
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
      in rec {
        defaultPackage = crane.lib.${system}.buildPackage {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            cargo-edit
            rustfmt
            rust-analyzer

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
        devShell = defaultPackage;
        devShell1 = pkgs.mkShell {
          name = "rust-env";
          src = ./.;

          # build-time deps
          # from https://blog.thomasheartman.com/posts/bevy-getting-started-on-nixos
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            rustfmt
            cargo-generate
            # fenix.packages.${system}.rust-analyzer
            rust-analyzer

            lld
            clang

            cargo-edit
            cargo-watch

            grpc-tools
            # eigen
            openmvg
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
          CPLUS_INCLUDE_PATH = "${pkgs.eigen}/include/eigen3";
        };
      });
}
