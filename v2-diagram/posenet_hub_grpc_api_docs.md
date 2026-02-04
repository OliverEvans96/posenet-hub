# Posenet Hub gRPC API Documentation

## Table of Contents
- [System Architecture](#system-architecture)
- [Client Types](#client-types)
- [Server Endpoints](#server-endpoints)
- [Communication Flows](#communication-flows)
- [Future Features](#future-features)

---

## System Architecture

The Posenet Hub uses a gRPC-based architecture with three types of clients communicating with a centralized server that exposes multiple endpoint groups.

### Endpoint Groups

| Group | Purpose | Color Code |
|-------|---------|------------|
| Camera | Camera authentication and control | Blue |
| Admin | System administration and monitoring | Pink |
| À la carte | Specialized computation services | Yellow |

---

## Client Types

### 1. Camera Client (gRPC Client)
**Purpose**: Physical camera nodes that capture and stream data to the hub

**Capabilities**:
- Authenticate with the hub
- Receive control commands
- Stream snapshot data
- Respond to ping/health checks

### 2. Admin Client (CLI/Web)
**Purpose**: Administrative interface for system management

**Capabilities**:
- List groups and cameras
- Control streaming operations
- Trigger snapshot captures
- Perform calibration
- Monitor system health

### 3. Jupyter Client
**Purpose**: Specialized client for computational tasks

**Capabilities**:
- Request triangulation computations
- Request bundle adjustment operations

---

## Server Endpoints

### Camera Endpoint Group

#### 1. Hello
**Purpose**: Camera authentication and session establishment

**Request**: `CameraInfo`
- `group_name`: string
- `camera_name`: string
- `calibration_data`: optional

**Response**: `SessionToken`

---

#### 2. CameraControl
**Purpose**: Send control commands to cameras

**Request Messages**:
- `SessionToken` (initial connection)
- `CommandToken` with `CameraControlCommand` enum

**Command Types**:
- `StartStreaming`
- `StopStreaming`
- `Ping`

**Response**: Command acknowledgment

---

#### 3. CameraDataSink
**Purpose**: Receive streaming data from cameras

**Request**: Stream of `Snapshot` messages
- `timestamp`: timestamp
- `camera`: camera identifier
- `poses`: optional pose data
- `image`: optional image data

**Initiated by**: `CommandToken` from CameraControl

**Stream Control**:
- Start: Send CommandToken
- Data: Continuous Snapshot messages
- End: End stream signal

---

### Admin Endpoint Group

#### 1. ListGroups
**Purpose**: Retrieve all camera groups in the system

**Request**: `ListGroupsRequest()`
- No parameters required

**Response**: `ListGroupsResponse`
- `group_names`: array of strings

---

#### 2. ListCameras
**Purpose**: Retrieve cameras within a specific group

**Request**: `ListCamerasRequest`
- `group_name`: string

**Response**: `ListCamerasResponse`
- `camera_names`: array of strings

---

#### 3. GetCameraInfo
**Purpose**: Get detailed information about a specific camera

**Request**: `CameraIdentifier`

**Response**: `CameraInfo`
- Camera configuration and status

---

#### 4. StreamControl
**Purpose**: Control streaming operations for camera groups

**Request**: `StreamControlRequest`
- `group_name`: string
- `command`: control command

**Response**: `StreamStatus`
- Current streaming state

---

#### 5. TakeSnapshots
**Purpose**: Trigger snapshot capture from camera group

**Request**: `ServerSnapshotRequest`
- `group_name`: string
- `params`: additional parameters (optional)

**Response**: `ServerSnapshotResponse`
- `id`: snapshot identifier
- `timestamp`: capture timestamp
- `messages`: associated messages
- `pose3d`: optional 3D pose data

---

#### 6. GetCurrent
**Purpose**: Retrieve current snapshot from a camera

**Request**: `CameraIdentifier`

**Response**: `ServerSnapshotResponse`
- `id`: snapshot identifier
- `timestamp`: capture timestamp
- `messages`: associated messages
- `pose3d`: optional 3D pose data

---

#### 7. Calibrate
**Purpose**: Perform camera calibration operations

**Request**: `CalibrationRequest`
- `which_camera`: camera identifier
- `command`: calibration command

**Response**: `CalibrationResponse`
- `calibration_status`: calibration result/status

---

#### 8. Ping
**Purpose**: Health check for cameras

**Request**: `PingRequest`
- `which_camera`: optional camera identifier
- `timeout`: timeout duration

**Response**: `PingResponse`
- `response_times`: optional timing data

---

### À la Carte Endpoint Group

#### 1. Triangulate
**Purpose**: Perform triangulation computations

**Request**: `TriangulationRequest`

**Response**: `TriangulationResponse`

---

#### 2. BundleAdjustment
**Purpose**: Perform bundle adjustment optimization

**Request**: `BARequest`

**Response**: `BAResponse`

---

## Communication Flows

### Flow 1: Camera Initialization and Streaming

```
Step 0: Camera → Hello
        CameraInfo(group_name, camera_name, calibration_data?)

Step 1: Hello → Camera
        SessionToken

Step 3: Camera → CameraControl
        SessionToken

Step 4: Camera → CameraControl
        CommandToken, CameraControlCommand.StartStreaming

Step 5: Camera → CameraDataSink
        CommandToken

Step 6: Camera → CameraDataSink
        stream Snapshot(timestamp, camera, poses?, image?)

Step 8: Camera → CameraDataSink
        end stream

Step 7: Camera → CameraControl
        CommandToken, CameraControlCommand.StopStreaming
```

### Flow 2: Camera Ping/Health Check

```
Step 9:  Camera → CameraControl
         CommandToken, CameraControlCommand.Ping

Step 10: Camera → CameraControl
         Ping(CommandToken)
```

### Flow 3: Admin Group Query

```
Admin → ListGroups
        ListGroupsRequest()

ListGroups → Admin
        ListGroupsResponse(group_names)

Admin → ListCameras
        ListCamerasRequest(group_name)

ListCameras → Admin
        ListCamerasResponse(camera_names)
```

### Flow 4: Admin Camera Operations

```
Admin → GetCameraInfo
        CameraIdentifier

GetCameraInfo → Admin
        CameraInfo

Admin → StreamControl
        StreamControlRequest(group_name, command)

StreamControl → Admin
        StreamStatus

Admin → TakeSnapshots
        ServerSnapshotRequest(group_name, params...)

TakeSnapshots → Admin
        ServerSnapshotResponse(id, timestamp, messages, pose3d?)
```

### Flow 5: Calibration

```
Admin → Calibrate
        CalibrationRequest(which_camera, command)

Calibrate → Admin
        CalibrationResponse(calibration_status)
```

### Flow 6: Jupyter Computation

```
Jupyter → Triangulate
         TriangulationRequest

Triangulate → Jupyter
         TriangulationResponse

Jupyter → BundleAdjustment
         BARequest

BundleAdjustment → Jupyter
         BAResponse
```

---

## Future Features

### TODO Items

#### High Priority
1. **CLI Authentication / RBAC Authorization**
   - Implement role-based access control
   - Add authentication for CLI and web interfaces

2. **Tail Stream Feature**
   - Equivalent to VRPN functionality
   - Implemented over gRPC

#### Low Priority: RecordControl Endpoint

**Purpose**: Recording and playback of camera data

**Methods**:

1. `StartRecording(group_name, recording_name?, duration?, params...)`
   - `params` can include: reverse, speed, loop, etc.
   
2. `StopRecording(group_name)`

3. `ReplayRecording(recording_id_or_name, params)`
   - `params` can include: reverse, speed, loop, etc.

4. `StopReplayingRecording(recording_id_or_name)`

#### Lower Priority: VRPNControl Endpoint

**Purpose**: VRPN device integration

**Methods**:

1. `SetVrpnParams(device_id, framerate?, ...)`

2. `StopVrpnDevice(device_id)`

3. `StartVrpnDevice(group_name, device_id)`

---

## Conventions and Notation

### Parameter Notation
- `?` suffix indicates optional parameter
- Example: `calibration_data?` means calibration_data is optional

### Message Types
- **Request messages**: Data sent from client to server
- **Response messages**: Data sent from server to client
- **Stream messages**: Continuous data flow (e.g., Snapshot stream)

### Token Types
- **SessionToken**: Issued during authentication, used for session management
- **CommandToken**: Issued for specific camera control commands

---

## Data Types Summary

| Type | Description | Fields |
|------|-------------|--------|
| CameraInfo | Camera identification and configuration | group_name, camera_name, calibration_data? |
| SessionToken | Authentication token | (opaque token) |
| CommandToken | Command authorization token | (opaque token) |
| CameraControlCommand | Camera command enum | StartStreaming, StopStreaming, Ping, etc. |
| Snapshot | Camera data snapshot | timestamp, camera, poses?, image? |
| CameraIdentifier | Unique camera reference | (camera identifier) |
| ServerSnapshotRequest | Snapshot request | group_name, params... |
| ServerSnapshotResponse | Snapshot data | id, timestamp, messages, pose3d? |
| CalibrationRequest | Calibration command | which_camera, command |
| CalibrationResponse | Calibration result | calibration_status |
| PingRequest | Health check request | which_camera?, timeout |
| PingResponse | Health check response | response_times? |
| StreamControlRequest | Stream control command | group_name, command |
| StreamStatus | Streaming state | (status information) |

---

## Notes

1. The system uses a token-based authentication and command pattern
2. Camera streaming is initiated via control commands and flows through dedicated data sink
3. Admin operations are separate from camera operations for clean separation of concerns
4. Specialized computation (triangulation, bundle adjustment) is provided via dedicated endpoints
5. Future enhancements include recording/playback and VRPN integration
