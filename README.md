# PoseNetVR Hub

Central hub written in Rust/C++ to manage Camera nodes. 

Collects 2D poses over gRPC, reconstructs 3D poses, and serves them over VRPN.

## Dependencies

The following must be available as system libraries:

- openMVG
- VRPN

## Configuration

The hub server can be configured via a TOML file. Use `--config path/to/config.toml` or set the `POSENET_CONFIG` environment variable. If no config file is provided, built-in defaults are used.

- **Sample config:** `config.toml.example` lists all options (grpc, http, websocket, vrpn, recording, triangulator, smoothing). Copy and edit as needed.
- **Recording directory:** Config `[recording] dir` or the `POSENET_RECORDING_DIR` environment variable (env overrides config).
- **WebSocket / VRPN:** Enable in config with `[websocket] enabled = true` or `[vrpn] enabled = true`, or at runtime with `--ws` and `--vrpn`.

## Development

### Docker

To use Docker for local development, run `./develop.sh`.
This will watch for changes to relevant files, and automatically rebuild the image and restart the container.

This requires `docker` and `nodemon` to be installed locally.

### Non-docker

```
cargo build
```

### VSCode Linting
To get linting working in VSCode, run the following:

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
