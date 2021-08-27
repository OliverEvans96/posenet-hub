# PoseNetVR Hub

Central hub written in Rust/C++ to manage Camera nodes. 

Collects 2D poses over gRPC, reconstructs 3D poses, and serves them over VRPN.

## Dependencies

The following must be available as system libraries:

- openMVG
- VRPN

## Development

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
