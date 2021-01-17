# PoseNetVR Hub

Central hub written in Rust/C++ to manage Camera nodes. 

Collects 2D poses over gRPC, reconstructs 3D poses, and serves them over VRPN.

## Dependencies

The following must be available as shared libraries:

- openMVG
- VRPN

## Development

To get linting working in VSCode, run the following:

```
cargo clean
bear -- cargo build
```

See https://github.com/dtolnay/cxx/issues/684
