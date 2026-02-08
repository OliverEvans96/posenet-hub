# PoseNetVR Hub

A distributed 3D pose estimation system built in Rust with C++ integration. The hub collects 2D pose observations from multiple camera nodes via gRPC, performs multi-view triangulation to reconstruct 3D poses, and streams results over WebSocket, HTTP, and optionally VRPN for VR tracking applications.

## Architecture

The system consists of several key components:

- **Hub Server** (Rust): Central coordinator that manages camera connections, performs 3D triangulation, and streams pose data
- **Camera Nodes**: Connect via gRPC to send 2D pose observations with calibration parameters
- **Synthetic Camera System**: Generates virtual camera feeds by rotating and projecting static poses to different viewpoints for testing
- **Triangulator**: Multi-view 3D reconstruction using openMVG with pose matching and Kalman filter-based smoothing
- **Frontend**: Real-time 3D visualization using Three.js with WebSocket connectivity
- **Controller**: Manages multiple camera groups and coordinate triangulation across concurrent sessions

### Data Flow

1. Cameras (real or synthetic) connect to the hub via gRPC and register with calibration parameters
2. Hub sends control commands to cameras to start/stop streaming
3. Cameras stream 2D pose observations (snapshots) to the hub
4. Triangulator matches poses across cameras, applies smoothing, and triangulates 3D poses
5. Results are broadcast via WebSocket to the frontend and optionally over VRPN for VR applications

## Current Status

The system architecture is solid and functional:

**Working:**
- ✅ Full 3D pose estimation pipeline from 2D observations to 3D reconstruction
- ✅ Synthetic camera system generating test data by rotating static poses and projecting them to multiple virtual viewpoints
- ✅ Synthetic cameras successfully connecting to the hub and streaming pose data
- ✅ Multi-camera triangulation with pose matching across views
- ✅ 2D pose smoothing using per-keypoint Kalman filters with temporal tracking
- ✅ Real-time WebSocket streaming to frontend
- ✅ Three.js-based frontend with 3D skeleton visualization and camera view panels
- ✅ HTTP/MJPEG streams for camera image feeds
- ✅ Recording and playback of pose sessions

**In Progress:**
- ⚠️ Real camera integration: The infrastructure is in place, but calibration accuracy needs improvement for optimal results
- ⚠️ Smoothing refinement: Kalman filter parameters need further tuning for real-world camera feeds
- ⚠️ Calibration workflow: While manual calibration files work, the process could be streamlined

The synthetic camera workflow demonstrates that the core triangulation and visualization pipeline is functioning correctly. Real camera support requires additional calibration refinement to achieve the same level of accuracy.

## Dependencies

### System Libraries

The following must be available:

- **openMVG** (v1.6): Multi-view geometry library for triangulation
- **VRPN**: Virtual Reality Peripheral Network (optional, for VR tracking output)
- **Eigen3**: Linear algebra library (used by openMVG)
- **Protocol Buffers**: For gRPC message definitions

### Managed by Nix

All dependencies are managed through Nix flakes (see `flake.nix`). The build system uses Nix to create reproducible Docker images with all required dependencies.

**Note:** Docker images are now built using Nix, not traditional Dockerfiles. The `flake.nix` defines both development environments and production Docker images.

## Configuration

The hub server can be configured via a TOML file. Use `--config path/to/config.toml` or set the `POSENET_CONFIG` environment variable. If no config file is provided, built-in defaults are used.

Example configuration (`config.toml`):

```toml
[grpc]
bind = "0.0.0.0"
port = 50051

[http]
bind = "0.0.0.0"
port = 9002

[websocket]
enabled = true
bind = "0.0.0.0"
port = 9001

[vrpn]
enabled = false
bind = "0.0.0.0"
port = 3883
device-name = "PoseNet0"

[recording]
dir = "recordings"

[triangulator]
pose-expiration-ms = 100
poll-interval-ms = 16
min-cameras = 2
pose-matching-threshold-px = 50.0

[smoothing]
r0 = 100.0
score-epsilon = 0.01
process-noise = 4.0
assoc-threshold-px = 120.0
min-score-observed = 0.2
hold-frames = 1
```

### Configuration Options

- **gRPC**: Camera control and data ingestion
- **HTTP**: MJPEG camera feed streaming
- **WebSocket**: Real-time pose stream for frontend
- **VRPN**: Optional VR tracking output
- **Recording**: Save pose sessions to disk for replay
- **Triangulator**: Multi-view reconstruction parameters
- **Smoothing**: Kalman filter parameters for 2D pose smoothing (optional; use `--no-smoothing` to disable)

## Development

### Using Nix (Recommended)

Enter the development shell with all dependencies:

```bash
nix develop
```

Or use the legacy shell:

```bash
nix-shell
```

Build the project:

```bash
cargo build
```

Build the Docker image using Nix:

```bash
nix build .#dockerImage
docker load < result
```

### Running the Hub Server

Start the hub with default configuration:

```bash
cargo run --bin hub-server
```

With custom config and WebSocket enabled:

```bash
cargo run --bin hub-server -- --config config.toml --ws
```

Enable VRPN output:

```bash
cargo run --bin hub-server -- --config config.toml --ws --vrpn
```

Disable smoothing (use raw poses):

```bash
cargo run --bin hub-server -- --no-smoothing
```

### Running Synthetic Cameras

The synthetic camera system is useful for testing the full pipeline without real cameras:

```bash
cargo run --bin run_synthetic_cameras -- --config cameras/<config-name>/config.yaml
```

Example synthetic camera configs are in the `cameras/` directory.

### Frontend Development

The frontend provides real-time 3D visualization of poses and camera views.

```bash
cd frontend
yarn install
yarn dev
```

Open http://localhost:3000 (connects to WebSocket on `ws://localhost:9001` by default).

See `frontend/README.md` for more details.

## Testing

Run the test suite:

```bash
cargo test
```

Frontend tests:

```bash
cd frontend
yarn test
```

## Deployment

### Building Docker Images with Nix

Production image (release build):

```bash
nix build .#dockerImage
docker load < result
```

Debug image (dev build with extra tools):

```bash
nix build .#dockerImageDebug
docker load < result
```

### Kubernetes

Example Kubernetes manifests are in the `k8s/` directory:

- `deploy.yaml`: Hub deployment
- `service.yaml`: Service definitions
- `vrpn-playback.yaml`: VRPN playback example

## API

### gRPC (Port 50051)

- `Hello(CameraInfo)`: Register a camera and receive a session token
- `CameraControl(SessionToken)`: Receive control commands (start/stop streaming)
- `CameraDataSink(stream CameraMessage)`: Upload pose snapshots from camera
- `StreamControl(StreamControlMessage)`: Admin API to start/stop streaming for camera groups

### WebSocket (Port 9001)

Real-time pose stream with 3D poses, per-camera 2D views, and camera models.

Message format:
```json
{
  "group_name": "default",
  "poses": [/* Pose3D objects */],
  "camera_views": [/* Per-camera 2D poses */],
  "cameras": [/* Camera models with position/orientation */],
  "timestamp": "..."
}
```

### HTTP (Port 9002)

- `/camera/<group>/<camera>/mjpeg`: MJPEG stream of camera feed
- `/recording/<group>/mjpeg`: MJPEG stream of recorded session

## Features

### Implemented

- ✅ Multi-camera 3D pose triangulation with openMVG
- ✅ Partial pose support (allows missing keypoints per camera)
- ✅ Pose matching across cameras using reprojection error
- ✅ Per-camera 2D pose smoothing with Kalman filters and temporal association
- ✅ Synthetic camera system for testing with rotating poses
- ✅ WebSocket streaming with 3D poses and camera views
- ✅ HTTP/MJPEG camera feed streaming
- ✅ VRPN output for VR tracking applications
- ✅ Session recording and playback
- ✅ Real-time frontend with Three.js 3D visualization
- ✅ Configuration hot-reload (Ctrl-C once to reload config)
- ✅ Multiple concurrent camera groups
- ✅ Docker image builds via Nix for reproducible deployments

### Future Improvements

- Camera auto-calibration and bundle adjustment for improved accuracy
- Enhanced multi-person tracking with robust pose association
- Real-time calibration refinement during operation
- Extended Kalman filter for 3D pose smoothing (currently only 2D)
- Temporal consistency across 3D poses
- Support for additional camera backends (RTSP, USB, etc.)

## Project Structure

```
.
├── src/
│   ├── bin/              # Binaries (hub-server, run_synthetic_cameras, etc.)
│   ├── grpc/             # gRPC server and protocol definitions
│   ├── triangulator/     # 3D reconstruction and pose matching
│   ├── synthetic_cameras/ # Synthetic camera pose generation
│   ├── websocket/        # WebSocket pose streaming
│   ├── http_server.rs    # HTTP/MJPEG streaming
│   ├── vrpn/             # VRPN tracker output
│   ├── recording/        # Session recording
│   └── controller.rs     # Camera group management
├── frontend/             # Three.js visualization frontend
├── proto/                # Protobuf definitions (submodule)
├── calibration/          # Example camera calibration files
├── cameras/              # Synthetic camera configurations
├── k8s/                  # Kubernetes deployment manifests
├── flake.nix             # Nix flake for builds and Docker images
├── Cargo.toml            # Rust dependencies
└── config.toml           # Example hub configuration
```

## License

See individual source files for license information.
