# PoseNetVR Hub

PoseNetVR Hub is a Rust/C++ service that collects 2D poses over gRPC, reconstructs
3D poses, and serves results over WebSocket and VRPN. It also exposes MJPEG camera
streams over HTTP and supports recording and replay.

## Current status

- End-to-end 3D pose estimation is working with synthetic data.
- Synthetic data is produced by taking a static pose, rotating it, and projecting
  into multiple viewpoints. Synthetic camera nodes connect to the hub the same way
  real cameras do.
- Real camera feeds are close but not fully reliable yet. Camera calibration needs
  improvement, pose smoothing needs refinement, and overall tuning is still in
  progress.
- The architecture is solid and the system is close to working end-to-end with
  real cameras.

## System overview

The hub is a gRPC server with camera and admin endpoints. It keeps per-group
triangulators that match poses across cameras, triangulate 3D keypoints (openMVG),
and optionally smooth 2D poses before matching. Pose updates are broadcast to:

- WebSocket clients as JSON (for the frontend and admin tools).
- VRPN server for VR device integration.
- HTTP MJPEG endpoints for camera feeds.

See `v2-diagram/` for protocol docs and diagrams.

## Repository layout

- `src/` Rust source (server, triangulator, websocket, http, recording, vrpn)
  - `src/bin/` CLI binaries (hub server, grpc client, synthetic cameras, replay)
  - `src/grpc/` gRPC server and client wrappers
  - `src/triangulator/` pose matching, triangulation, and smoothing
  - `src/synthetic_cameras/` synthetic camera generation and projection
  - `src/websocket/` JSON pose stream and admin commands
  - `src/http_server.rs` MJPEG camera stream and recording download
  - `src/openmvg/` C++ bridge (openMVG) for triangulation and bundle adjustment
  - `src/vrpn/` C++ bridge (VRPN) for VR device integration
- `include/` C++ headers for cxx bridges (openMVG, Eigen, VRPN)
- `fake_poses/` sample pose CSV and synthetic camera YAML config
- `frontend/` Three.js pose viewer (Vite)
- `v2-diagram/` gRPC API docs and text diagrams
- `build/` legacy Dockerfiles and configs (not used for current builds)
- `proto/` gRPC protobuf submodule (see `.gitmodules`)

## Services and default ports

- gRPC hub: `0.0.0.0:50051`
- WebSocket pose stream: `0.0.0.0:9001`
- HTTP MJPEG and recording download: `0.0.0.0:9002`
- VRPN: `0.0.0.0:3883`

Defaults are in `config.toml`.

## Binaries and CLI tools

### hub-server

Main hub service (gRPC + optional WebSocket + optional VRPN + HTTP MJPEG).

```
cargo run --bin hub-server -- --config config.toml --ws --vrpn
```

Notes:
- `--config` or `POSENET_CONFIG` points to the TOML config.
- First Ctrl-C reloads config when a config file is provided; second Ctrl-C exits.
- `--no-smoothing` disables 2D smoothing.

### grpc-client

Admin and camera CLI for testing the gRPC API.

Admin commands:
- `list-groups`, `list-cameras`, `get-camera-info`
- `stream-control-start`, `stream-control-stop`
- `take-snapshots`, `get-current`
- `calibrate`, `ping`, `update-cameras`

Example:
```
cargo run --bin grpc-client -- admin list-groups --server localhost --port 50051
```

Camera commands:
- `stream-poses` (random poses)
- `offer-snapshots` (respond to snapshot requests)

### run_synthetic_cameras

Streams synthetic camera data to the hub by rotating a static pose and projecting
to multiple virtual cameras. Uses YAML config:

```
cargo run --bin run_synthetic_cameras -- --config fake_poses/synthetic_cameras_config.yaml
```

### replay_recording

Replays a `.pnhr` recording into the hub as if cameras were streaming.

```
cargo run --bin replay_recording -- --file recordings/recording_*.pnhr --realtime
```

### vrpn-client / vrpn-playback

Test VRPN integration or replay poses to VRPN.

## Frontend (new)

`frontend/` is a real-time 3D pose visualization frontend (Three.js). It consumes
the WebSocket pose stream and can send admin commands (list groups/cameras,
start/stop streaming, take snapshots, start/stop recording).

```
cd frontend
yarn install
yarn dev
```

Default WebSocket URL: `ws://localhost:9001` (override with `?ws=PORT`).
See `frontend/README.md` for details and tests.

## WebSocket and HTTP APIs

### WebSocket pose stream

Each message is JSON with:
- `group_name`
- `poses` (3D poses, 17 keypoints)
- `camera_views` (per-camera 2D poses)
- `cameras` (camera models for visualization)
- `timestamp_ms`

### WebSocket admin commands (JSON-RPC style)

Request: `{ "id": 1, "method": "ListGroups", "params": { ... } }`
Response: `{ "id": 1, "result": { ... } }` or `{ "id": 1, "error": "..." }`

Supported methods (see `src/websocket/server.rs` and `frontend/src/adminApi.js`):
`ListGroups`, `ListCameras`, `GetCameraInfo`, `StreamControl`, `TakeSnapshots`,
`GetCurrent`, `Calibrate`, `Ping`, `UpdateCameras`, `StartRecording`,
`StopRecording`, `GetRecordingStatus`.

### HTTP MJPEG and recordings

- MJPEG stream: `GET /api/camera/{group}/{camera}/stream`
- Download last recording: `GET /api/recordings/download`

## Recording and replay

The hub can record raw snapshots (poses and images) to `.pnhr` files. Recording
can be controlled over the WebSocket admin API. Recordings are written before any
server-side smoothing so they can be replayed deterministically.

Use `replay_recording` to inject a `.pnhr` file into the hub.

## Configuration

The hub server uses a TOML config (`config.toml` is a runnable example). Provide
`--config path/to/config.toml` or set `POSENET_CONFIG`. If no config file is
provided, defaults are used.

Key sections:
- `[grpc]`, `[http]`, `[websocket]`, `[vrpn]`
- `[recording] dir` (or `POSENET_RECORDING_DIR`)
- `[triangulator]` and `[smoothing]` (pose matching, smoothing parameters)

## Build and development

### Nix (recommended)

Dependencies and dev shell are managed via `flake.nix`:

```
nix develop
cargo build
```

The flake builds openMVG 1.6, Eigen, VRPN, and protobuf. It also builds a Docker
image (see below). The `proto/` submodule and LFS assets are expected in a clean
clone:

```
git submodule update --init proto
git lfs pull
```

### Docker image (via Nix)

Docker images are built with Nix. The Dockerfiles in `build/` are legacy and not
used for the current workflow.

```
nix build -L .#dockerImage
docker load < result
docker run -p 50051:50051 -p 3883:3883 posenet-docker:latest
```

For a debug image with extra tooling, use `.#dockerImageDebug`.

### Non-Nix build

If you are not using Nix, install system libraries and point the build to them:

- openMVG, VRPN, protobuf, Eigen
- `EIGEN_INCLUDE_DIR`, `OMVG`, and `PROTOC` may be required

```
cargo build
```

### VSCode linting (non-Nix)

```
cargo clean
bear -- cargo build
```

See https://github.com/dtolnay/cxx/issues/684

## Tests

Rust:
```
cargo test
```

Frontend:
```
cd frontend
yarn test
```

## Known gaps and next steps

- Improve camera calibration for real-world feeds (intrinsics and extrinsics).
- Refine pose smoothing to reduce jitter and latency.
- Support missing keypoints and confidence handling in 3D reconstruction.
- Support multiple poses and cross-camera pose association.
- Consider camera intrinsics details (sensor model, distortion).
- Automate camera position/orientation estimation.
- Perform bundle adjustment for improved camera matrices and 3D points.
