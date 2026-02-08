# PoseNetVR Hub

Central hub written in Rust/C++ to manage Camera nodes. 

Collects 2D poses over gRPC, reconstructs 3D poses, and serves them over VRPN.

## Current Status

- End-to-end 3D pose estimation is working with synthetic data.
- Synthetic data is produced by taking a static pose, rotating it, and projecting it
  into multiple viewpoints. Synthetic camera nodes connect to the hub the same way
  real cameras do.
- Real camera feeds are close but not fully reliable yet. Camera calibration needs
  improvement, pose smoothing needs refinement, and overall tuning is still in
  progress.
- The architecture is solid and the system is close to working end-to-end with
  real cameras.

## Dependencies

Dependencies are managed via Nix (see `flake.nix`) and are recommended for local
development. If you are not using Nix, the following must be available as system
libraries:

- openMVG
- VRPN
- protobuf (protoc)

## Configuration

The hub server can be configured via a TOML file. Use `--config path/to/config.toml` or set the `POSENET_CONFIG` environment variable. If no config file is provided, built-in defaults are used.

- **Sample config:** `config.toml.example` lists all options (grpc, http, websocket, vrpn, recording, triangulator, smoothing). Copy and edit as needed.
- **Recording directory:** Config `[recording] dir` or the `POSENET_RECORDING_DIR` environment variable (env overrides config).
- **WebSocket / VRPN:** Enable in config with `[websocket] enabled = true` or `[vrpn] enabled = true`, or at runtime with `--ws` and `--vrpn`.

## Frontend

There is a new real-time 3D pose visualization frontend in `frontend/` (Three.js).
It connects to the hub's WebSocket pose stream.

```
cd frontend
yarn install
yarn dev
```

By default it connects to `ws://localhost:9001`. See `frontend/README.md` for
details, build steps, and tests.

## Development

### Nix (recommended)

```
nix develop
cargo build
```

### Docker image (via Nix)

The Docker image is built with Nix (Dockerfiles in `build/` are legacy and not
used for the current workflow).

```
nix build -L .#dockerImage
docker load < result
docker run -p 50051:50051 -p 3883:3883 posenet-docker:latest
```

For a debug image with extra tooling, use `.#dockerImageDebug`.

### Non-Nix build

```
cargo build
```

### VSCode Linting
To get linting working in VSCode, run the following (non-Nix users):

```
cargo clean
bear -- cargo build
```

See https://github.com/dtolnay/cxx/issues/684

# TODO

This is a first draft. There are a lot of improvements that will be needed. Including, but not limited to:

- Allow missing keypoints, and/or keypoint score values for 3d pose
- Support multiple poses, will need a way to correlate poses across different cameras
- Consider [camera intrinsics](https://en.wikipedia.org/wiki/Camera_resectioning#Intrinsic_parameters) (focal length, image sensor format, and principal point, and radial distortion?)
- Determine/adjust camera position/orientation automatically.
- Perform [bundle adjustment](https://openmvg.readthedocs.io/en/latest/openMVG/sfm/sfm/#non-linear-refinement-bundle-adjustment) to get accurate camera matrices & 3d points.
- Improve camera calibration for real-world feeds.
- Refine pose smoothing to reduce jitter and latency.
