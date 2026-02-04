# Admin CLI (grpc-client) – Assessment & Implementation Plan

This document assesses the current state of the `grpc-client` binary as the **Admin Client** from the v2 diagram and outlines a plan to implement all Admin endpoint features (excluding the TODO/Future Features section).

---

## 1. V2 Diagram Admin Requirements (Non-TODO)

From `v2-diagram/posenet_hub_grpc_api_docs.md` and `posenet_hub_grpc_diagram.txt`, the Admin client must support:

| # | Endpoint       | Request                                      | Response                                      | Purpose                          |
|---|----------------|----------------------------------------------|-----------------------------------------------|----------------------------------|
| 1 | **ListGroups** | `ListGroupsRequest()` (empty)                | `ListGroupsResponse(group_names)`             | List all camera groups           |
| 2 | **ListCameras**| `ListCamerasRequest(group_name)`             | `ListCamerasResponse(camera_names)`            | List cameras in a group          |
| 3 | **GetCameraInfo** | `CameraIdentifier` (group + camera)       | `CameraInfo`                                  | Get detailed info for one camera |
| 4 | **StreamControl** | `StreamControlRequest(group_name, command)` | `StreamStatus`                                | Start/stop streaming per group   |
| 5 | **TakeSnapshots** | `ServerSnapshotRequest(which_camera, params, want_pose3d)` | `ServerSnapshotResponse`       | Trigger snapshot capture         |
| 6 | **GetCurrent**  | `CameraIdentifier` (group, optional camera)  | `ServerSnapshotResponse`                      | Get latest cached snapshot       |
| 7 | **Calibrate**   | `CalibrationRequest(which_camera, command)`  | `CalibrationResponse(states)`                  | Run calibration                  |
| 8 | **Ping**       | `PingRequest(which_camera?, timeout)`         | `PingResponse(results)`                       | Health check                     |

Proto types (from `proto/hub.proto`, `proto/camera.proto`):

- **CameraIdentifier**: `group_name`, `camera_name` (empty `camera_name` = whole group where applicable).
- **ServerSnapshotRequest**: `which_camera`, `params` (SnapshotParameters), `want_pose3d`.
- **ServerSnapshotResponse**: `snapshot_id`, `timestamp`, `snapshots` (repeated Snapshot), `pose3d`.
- **StreamControlRequest**: `group_name`, `command` oneof: `StartStreaming(StreamParameters)` or `StopStreaming`.
- **StreamStatus**: `is_streaming`, `params` (optional).
- **CalibrationRequest**: `which_camera`, `command` (CalibrateCommand: do_extrinsic, do_intrinsic).
- **PingRequest**: `which_camera` (optional), `timeout` (Duration).

---

## 2. Current State Assessment

### 2.1 Binary layout (`src/bin/grpc-client.rs`)

- **Common options**: `--port` (default 50051), `--server` (default localhost). Used for connection.
- **Top-level commands**:
  - **Camera** (camera client role): `StreamPoses`, `OfferSnapshots` — kept as-is for camera simulation; not part of “admin” feature set.
  - **GetSnapshots**: calls `todo!()` — intended to map to **TakeSnapshots** + save output.
  - **GetSnapshotCameras**: calls `todo!()` — old behaviour wrote intrinsics/extrinsics (effectively **ListCameras** + **GetCameraInfo** per camera); can be replaced by explicit admin commands.

**Missing from diagram:**

- No **ListGroups** / **ListCameras**.
- No **GetCameraInfo** (single camera).
- No **StreamControl** (start/stop streaming).
- No **GetCurrent** (latest snapshot from cache).
- No **Calibrate**.
- No **Ping**.
- **TakeSnapshots** is only stubbed (GetSnapshots).

So the CLI currently implements **0 of 8** Admin flows as first-class commands.

### 2.2 Library layer (`src/grpc/client.rs`)

- **Camera client**: `hello`, `stream_poses` (with internal `todo!()`), `offer_snapshots` (with internal `todo!()`). Used by the Camera subcommands.
- **Admin-related**:
  - `get_snapshots`: builds `CameraIdentifier`, then `todo!()` — never calls `TakeSnapshots`.
  - `get_snapshot_cameras`: builds `ServerSnapshotRequest` correctly, then `todo!()` — never calls the client.

The generated `HubServiceClient` (tonic) provides all 8 admin RPCs as methods (e.g. `list_groups`, `list_cameras`, `get_camera_info`, `stream_control`, `take_snapshots`, `get_current`, `calibrate`, `ping`). The library does not expose wrappers for these; the binary does not call them.

### 2.3 Server alignment

- All 8 Admin RPCs are implemented in `src/grpc/server.rs` with the proto types above.
- Behaviour: list groups from `session_tokens`, list cameras per group, get camera info from `camera_info`, stream control via `execute_command`, take_snapshots/get_current/calibrate/ping as in the diagram. No API gaps for the plan.

### 2.4 Dependencies and CLI parsing

- **structopt** is used; `Cargo.toml` also has **clap** with derive. Optional: migrate to **clap** v3/v4 derive for a single CLI stack and better long-term maintenance.
- **anyhow** + **env_logger** + **dotenv** are in place for errors and logging.

---

## 3. Implementation Plan

### Phase 1: Library – Admin RPC wrappers

**Goal:** In `src/grpc/client.rs`, add thin async wrappers that call the generated `HubServiceClient` for each Admin RPC and map errors consistently (e.g. to `Box<dyn Error + Send + Sync>` or `anyhow::Result`).

1. **ListGroups**  
   - Input: none (or accept `&mut HubServiceClient` only).  
   - Call `client.list_groups(ListGroupsRequest {})`.  
   - Return `ListGroupsResponse` (or `Vec<String>`).

2. **ListCameras**  
   - Input: `group_name: String`.  
   - Call `client.list_cameras(ListCamerasRequest { group_name })`.  
   - Return `ListCamerasResponse` (or `Vec<String>`).

3. **GetCameraInfo**  
   - Input: `CameraIdentifier` (group_name, camera_name).  
   - Call `client.get_camera_info(identifier)`.  
   - Return `CameraInfo`.

4. **StreamControl**  
   - Input: `StreamControlRequest` (group_name + command: start with params or stop).  
   - Call `client.stream_control(request)`.  
   - Return `StreamStatus`.

5. **TakeSnapshots**  
   - Input: `ServerSnapshotRequest` (which_camera, params, want_pose3d).  
   - Call `client.take_snapshots(request)`.  
   - Return `ServerSnapshotResponse`.  
   - Replace/implement the current `get_snapshots` logic (build request, call TakeSnapshots, return response).

6. **GetCurrent**  
   - Input: `CameraIdentifier` (group, optional camera name).  
   - Call `client.get_current(identifier)`.  
   - Return `ServerSnapshotResponse`.

7. **Calibrate**  
   - Input: `CalibrationRequest` (which_camera, command).  
   - Call `client.calibrate(request)`.  
   - Return `CalibrationResponse`.

8. **Ping**  
   - Input: `PingRequest` (optional which_camera, optional timeout).  
   - Call `client.ping(request)`.  
   - Return `PingResponse`.

Remove or replace the existing `get_snapshots` / `get_snapshot_cameras` stubs so that all admin behaviour goes through these wrappers and the proto types.

---

### Phase 2: CLI – Subcommands and flags

**Goal:** One top-level “Admin” mode with subcommands for each Admin endpoint, plus clear grouping and shared options.

1. **Shared connection options**  
   - Keep `CommonOpts`: `--server`, `--port`.  
   - Use them for all Admin commands.

2. **Camera identifier handling**  
   - For commands that need a camera or group:  
     - `--group` (required where needed).  
     - `--camera` (optional; if omitted, use empty string = whole group where the server supports it).

3. **Top-level structure**  
   - Keep **Camera** as a top-level variant (camera client): `StreamPoses`, `OfferSnapshots`.  
   - Add **Admin** (or equivalent name) as a top-level variant with subcommands:

     - **list-groups**  
       - No extra args (or just common).  
       - Call list_groups; print group names (one per line or table).

     - **list-cameras**  
       - `--group <name>`.  
       - Call list_cameras; print camera names.

     - **get-camera-info**  
       - `--group <name>` and `--camera <name>`.  
       - Call get_camera_info; print CameraInfo (e.g. YAML/JSON or readable summary).

     - **stream-control**  
       - Sub-subcommands or flags:  
         - **start**: `--group`, optional `--with-pose`, `--with-image`, `--fps`.  
         - **stop**: `--group`.  
       - Build `StreamControlRequest` and `StreamParameters` from flags; call stream_control; print StreamStatus.

     - **take-snapshots**  
       - `--group`, optional `--camera`, `--with-pose`, `--with-image`, `--want-pose3d`.  
       - Build `ServerSnapshotRequest` (which_camera, params, want_pose3d); call take_snapshots; then either print summary (snapshot_id, timestamp, count) and optionally save snapshots/pose3d to a directory (current GetSnapshots behaviour).

     - **get-current**  
       - `--group`, optional `--camera`.  
       - Call get_current; print summary and optionally save to directory (same output shape as take-snapshots if desired).

     - **calibrate**  
       - `--group`, optional `--camera`, `--do-extrinsic`, `--do-intrinsic`.  
       - Build CalibrationRequest; call calibrate; print CalibrationResponse (e.g. states per camera).

     - **ping**  
       - Optional `--group`, optional `--camera`, optional `--timeout`.  
       - Build PingRequest (which_camera = None if no group/camera); call ping; print response times or “no cameras specified”.

4. **GetSnapshots / GetSnapshotCameras**  
   - **GetSnapshots**: Implement by calling the new TakeSnapshots wrapper and then saving `ServerSnapshotResponse` to the existing output dir (snapshot_id subdir, images/poses/pose3d).  
   - **GetSnapshotCameras**: Either remove or reinterpret as “list cameras + get camera info for each” using ListCameras + GetCameraInfo (and optionally save intrinsics/extrinsics to disk as in the old comment block). Prefer making this explicit as admin subcommands (list-cameras, get-camera-info) plus optional “save to dir” behaviour.

5. **Output format**  
   - Decide default: human-readable (tables, one-per-line) vs machine-readable (JSON).  
   - Optional `--output json` (or similar) for scripts.  
   - For snapshot responses: keep existing “write to directory” behaviour where it makes sense (take-snapshots, get-current).

---

### Phase 3: Output and UX

1. **ListGroups / ListCameras**  
   - Print names (one per line or table).  
   - On error (e.g. group not found for list_cameras), print clear message and exit with non-zero.

2. **GetCameraInfo**  
   - Print calibration and identifier; support optional `--output dir` to write intrinsics/extrinsics YAML as in the old GetSnapshotCameras comment.

3. **StreamControl**  
   - Print “Streaming started/stopped” and StreamStatus (is_streaming, params).

4. **TakeSnapshots / GetCurrent**  
   - Print snapshot_id, timestamp, number of snapshots, pose3d presence.  
   - If `--output <dir>` (or existing `output` from current GetSnapshots): create `<dir>/<snapshot_id>/`, write images and pose data per snapshot, plus pose3d if present.

5. **Calibrate**  
   - Print each calibration state (which_camera, calibration if present).

6. **Ping**  
   - Print per-camera response time; if which_camera omitted, print that no cameras were pinged (matching server behaviour).

7. **Errors**  
   - Use anyhow for context; map tonic/Status to user-friendly messages (e.g. “camera not found”, “group not found”, “no current snapshot”).

---

### Phase 4: Cleanup and consistency

1. **grpc/client.rs**  
   - Remove all `todo!()` from admin-related paths.  
   - Ensure stream_poses / offer_snapshots are either fully implemented or clearly marked as camera-client-only stubs (and leave them for a separate camera-client task if needed).

2. **grpc-client.rs**  
   - No `todo!()` in main match; every branch either runs a command or exits with a clear “not implemented” message.  
   - Share one `connect(server, port)` and pass `HubServiceClient` into library wrappers.

3. **Optional: structopt → clap**  
   - Migrate to clap derive (Subcommand, Args) and remove structopt to match Cargo.toml and modern usage.

4. **Docs**  
   - Update README or v2 docs to state that `grpc-client` is the Admin CLI and list the 8 admin subcommands and Camera subcommands.

---

## 4. Suggested Order of Implementation

1. **Library (Phase 1)**  
   - Implement the 8 admin wrappers in `src/grpc/client.rs` and fix/remove get_snapshots and get_snapshot_cameras.
2. **CLI structure (Phase 2)**  
   - Add Admin top-level and subcommands (list-groups, list-cameras, get-camera-info, stream-control, take-snapshots, get-current, calibrate, ping); wire each to the corresponding wrapper.
3. **TakeSnapshots + GetCurrent output (Phase 3)**  
   - Implement directory output for take-snapshots and get-current (reuse/adapt current GetSnapshots output logic).
4. **Remaining UX (Phase 3)**  
   - Formatting, error messages, optional JSON output.
5. **Cleanup (Phase 4)**  
   - Remove todos, optional clap migration, docs.

---

## 5. Out of Scope (per “skip TODO section”)

- CLI Authentication / RBAC  
- Tail stream (VRPN-like over gRPC)  
- RecordControl / VRPNControl endpoints  

These are left for future work and not part of this plan.

---

## 6. Summary

| Area              | Current state                         | Target state                                                |
|-------------------|----------------------------------------|-------------------------------------------------------------|
| Admin RPCs in lib | 0 implemented (2 stubs with todo)     | 8 wrappers: list_groups, list_cameras, get_camera_info, stream_control, take_snapshots, get_current, calibrate, ping |
| CLI Admin commands| 0 (GetSnapshots/GetSnapshotCameras todo)| 8 subcommands under Admin + TakeSnapshots/GetCurrent output to dir |
| Server alignment  | N/A                                    | Use same proto types and semantics as server               |
| Camera commands   | Present (StreamPoses, OfferSnapshots)  | Keep as-is for camera client role                          |

This plan brings the grpc-client binary to a solid Admin CLI that matches the server implementation and covers all non-TODO Admin features from the v2 diagram.
