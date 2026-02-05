//! WebSocket server that forwards pose broadcast messages to connected clients as JSON
//! and handles admin commands (JSON-RPC style) when an admin hub is provided.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};
use prost_types;
use tonic::Request;

use crate::grpc::proto::{
    stream_control_request, CalibrateCommand, CalibrationRequest, CameraIdentifier,
    CameraInfo, CalibrationResponse, ListCamerasRequest, ListGroupsRequest, PingRequest,
    PingResponse, Point2D, Point3D, Pose2D, Pose3D, ServerSnapshotRequest, ServerSnapshotResponse,
    SnapshotParameters, SnapshotPayloadParameters, StartStreamingRequest, StreamControlRequest,
    StreamParameters, StreamStatus, StopStreamingRequest, UpdateCamerasRequest,
};
use crate::grpc::proto::hub_service_server::HubService;
use crate::grpc::server::HubServer;
use crate::triangulator::{CameraModel, CameraView, PoseStreamUpdate};

/// JSON-serializable 3D point for WebSocket clients.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Point3DJson {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub score: f64,
}

/// JSON-serializable 3D pose: 17 keypoints in standard order (nose, left_eye, ...).
/// Missing keypoints are serialized as null.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Pose3DJson {
    pub keypoints: Vec<Option<Point3DJson>>,
    pub score: f64,
}

/// JSON-serializable 2D point (camera view keypoint).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Point2DJson {
    pub x: f64,
    pub y: f64,
    pub score: f64,
}

/// JSON-serializable 2D pose: 17 keypoints in standard order.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Pose2DJson {
    pub keypoints: Vec<Option<Point2DJson>>,
    pub score: f64,
}

/// Per-camera 2D view for WebSocket clients.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CameraViewJson {
    pub camera_name: String,
    pub poses: Vec<Pose2DJson>,
}

/// Camera pose/orientation for 3D visualization.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CameraModelJson {
    pub camera_name: String,
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    pub width_px: f64,
    pub height_px: f64,
    pub position: Point3DJson,
    pub right: Point3DJson,
    pub up: Point3DJson,
    pub forward: Point3DJson,
}

/// Message sent over WebSocket for each pose update.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PoseStreamMessage {
    pub group_name: String,
    pub poses: Vec<Pose3DJson>,
    /// Per-camera 2D poses for camera view panels.
    pub camera_views: Vec<CameraViewJson>,
    /// Camera models (pose/orientation) for drawing cameras in the 3D view.
    pub cameras: Vec<CameraModelJson>,
    /// Unix timestamp in milliseconds when the message was sent.
    pub timestamp_ms: u64,
}

fn point3d_to_json(p: &Point3D) -> Point3DJson {
    Point3DJson {
        x: p.x,
        y: p.y,
        z: p.z,
        score: p.score,
    }
}

fn pose3d_keypoints(pose: &Pose3D) -> [Option<&Point3D>; 17] {
    [
        pose.nose.as_ref(),
        pose.left_eye.as_ref(),
        pose.right_eye.as_ref(),
        pose.left_ear.as_ref(),
        pose.right_ear.as_ref(),
        pose.left_shoulder.as_ref(),
        pose.right_shoulder.as_ref(),
        pose.left_elbow.as_ref(),
        pose.right_elbow.as_ref(),
        pose.left_wrist.as_ref(),
        pose.right_wrist.as_ref(),
        pose.left_hip.as_ref(),
        pose.right_hip.as_ref(),
        pose.left_knee.as_ref(),
        pose.right_knee.as_ref(),
        pose.left_ankle.as_ref(),
        pose.right_ankle.as_ref(),
    ]
}

fn pose3d_to_json(pose: &Pose3D) -> Pose3DJson {
    Pose3DJson {
        keypoints: pose3d_keypoints(pose)
            .iter()
            .map(|opt| opt.map(point3d_to_json))
            .collect(),
        score: pose.score,
    }
}

fn point2d_to_json(p: &Point2D) -> Point2DJson {
    Point2DJson {
        x: p.x,
        y: p.y,
        score: p.score,
    }
}

fn pose2d_keypoints(pose: &Pose2D) -> [Option<&Point2D>; 17] {
    [
        pose.nose.as_ref(),
        pose.left_eye.as_ref(),
        pose.right_eye.as_ref(),
        pose.left_ear.as_ref(),
        pose.right_ear.as_ref(),
        pose.left_shoulder.as_ref(),
        pose.right_shoulder.as_ref(),
        pose.left_elbow.as_ref(),
        pose.right_elbow.as_ref(),
        pose.left_wrist.as_ref(),
        pose.right_wrist.as_ref(),
        pose.left_hip.as_ref(),
        pose.right_hip.as_ref(),
        pose.left_knee.as_ref(),
        pose.right_knee.as_ref(),
        pose.left_ankle.as_ref(),
        pose.right_ankle.as_ref(),
    ]
}

fn pose2d_to_json(pose: &Pose2D) -> Pose2DJson {
    Pose2DJson {
        keypoints: pose2d_keypoints(pose)
            .iter()
            .map(|opt| opt.map(point2d_to_json))
            .collect(),
        score: pose.score,
    }
}

fn camera_view_to_json(v: &CameraView) -> CameraViewJson {
    CameraViewJson {
        camera_name: v.camera_name.clone(),
        poses: v.poses.iter().map(pose2d_to_json).collect(),
    }
}

fn vec3_to_point3_json(v: &nalgebra::Vector3<f64>) -> Point3DJson {
    Point3DJson {
        x: v.x,
        y: v.y,
        z: v.z,
        score: 1.0,
    }
}

fn point3_to_point3_json(p: &nalgebra::Point3<f64>) -> Point3DJson {
    Point3DJson {
        x: p.x,
        y: p.y,
        z: p.z,
        score: 1.0,
    }
}

fn camera_model_to_json(c: &CameraModel) -> CameraModelJson {
    CameraModelJson {
        camera_name: c.camera_name.clone(),
        fx: c.fx,
        fy: c.fy,
        cx: c.cx,
        cy: c.cy,
        width_px: c.width_px,
        height_px: c.height_px,
        position: point3_to_point3_json(&c.position),
        right: vec3_to_point3_json(&c.right),
        up: vec3_to_point3_json(&c.up),
        forward: vec3_to_point3_json(&c.forward),
    }
}

fn stream_update_to_message(update: &PoseStreamUpdate) -> PoseStreamMessage {
    PoseStreamMessage {
        group_name: update.labeled_poses.group_name.clone(),
        poses: update.labeled_poses.poses.iter().map(pose3d_to_json).collect(),
        camera_views: update.camera_views.iter().map(camera_view_to_json).collect(),
        cameras: update.cameras.iter().map(camera_model_to_json).collect(),
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}

/// Handle one admin request and return JSON response string (result or error).
async fn handle_ws_admin(hub: &HubServer, req: WsAdminRequest) -> String {
    let id = req.id;
    let result = match req.method.as_str() {
        "ListGroups" => {
            let r = hub
                .list_groups(Request::new(ListGroupsRequest {}))
                .await
                .map(|r| {
                    let inner = r.into_inner();
                    serde_json::json!({ "group_names": inner.group_names })
                });
            r
        }
        "ListCameras" => {
            let group_name = req
                .params
                .and_then(|p| p.get("group_name").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_default();
            let r = hub
                .list_cameras(Request::new(ListCamerasRequest { group_name }))
                .await
                .map(|r| {
                    let inner = r.into_inner();
                    serde_json::json!({ "camera_names": inner.camera_names })
                });
            r
        }
        "GetCameraInfo" => {
            let (group_name, camera_name) = parse_camera_identifier(&req.params);
            let which = CameraIdentifier {
                group_name,
                camera_name,
            };
            let r = hub
                .get_camera_info(Request::new(which))
                .await
                .map(|r| camera_info_to_json(r.into_inner()));
            r
        }
        "StreamControl" => {
            let params = req.params.as_ref();
            let group_name = params
                .and_then(|p| p.get("group_name").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_default();
            let command = params.and_then(|p| p.get("command").cloned());
            let stream_req = if command.as_ref().and_then(|c| c.get("start")).is_some() {
                let start_params = command
                    .as_ref()
                    .and_then(|c| c.get("start").cloned())
                    .unwrap_or(serde_json::Value::Null);
                let with_pose = start_params.get("with_pose").and_then(|v| v.as_bool()).unwrap_or(true);
                let with_image = start_params.get("with_image").and_then(|v| v.as_bool()).unwrap_or(true);
                let fps = start_params.get("fps").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                StreamControlRequest {
                    group_name: group_name.clone(),
                    command: Some(stream_control_request::Command::StartStreaming(
                        StartStreamingRequest {
                            group_name,
                            params: Some(StreamParameters {
                                common: Some(SnapshotPayloadParameters {
                                    with_pose,
                                    with_image,
                                }),
                                fps,
                            }),
                        },
                    )),
                }
            } else if command.as_ref().and_then(|c| c.get("stop")).is_some() {
                StreamControlRequest {
                    group_name: group_name.clone(),
                    command: Some(stream_control_request::Command::StopStreaming(
                        StopStreamingRequest { group_name },
                    )),
                }
            } else {
                return serde_json::json!({ "id": id, "error": "StreamControl requires command.start or command.stop" }).to_string();
            };
            let r = hub
                .stream_control(Request::new(stream_req))
                .await
                .map(|r| stream_status_to_json(r.into_inner()));
            r
        }
        "TakeSnapshots" => {
            let (group_name, camera_name) = parse_camera_identifier(&req.params);
            let with_pose = req
                .params
                .as_ref()
                .and_then(|p| p.get("with_pose").and_then(|v| v.as_bool()))
                .unwrap_or(true);
            let with_image = req
                .params
                .as_ref()
                .and_then(|p| p.get("with_image").and_then(|v| v.as_bool()))
                .unwrap_or(false);
            let want_pose3d = req
                .params
                .as_ref()
                .and_then(|p| p.get("want_pose3d").and_then(|v| v.as_bool()))
                .unwrap_or(false);
            let which = CameraIdentifier {
                group_name,
                camera_name,
            };
            let snap_req = ServerSnapshotRequest {
                which_camera: Some(which),
                params: Some(SnapshotParameters {
                    common: Some(SnapshotPayloadParameters {
                        with_pose,
                        with_image,
                    }),
                }),
                want_pose3d,
            };
            let r = hub
                .take_snapshots(Request::new(snap_req))
                .await
                .map(|r| server_snapshot_response_to_json(r.into_inner()));
            r
        }
        "GetCurrent" => {
            let (group_name, camera_name) = parse_camera_identifier(&req.params);
            let which = CameraIdentifier {
                group_name,
                camera_name,
            };
            let r = hub
                .get_current(Request::new(which))
                .await
                .map(|r| server_snapshot_response_to_json(r.into_inner()));
            r
        }
        "Calibrate" => {
            let (group_name, camera_name) = parse_camera_identifier(&req.params);
            let do_extrinsic = req
                .params
                .as_ref()
                .and_then(|p| p.get("do_extrinsic").and_then(|v| v.as_bool()))
                .unwrap_or(false);
            let do_intrinsic = req
                .params
                .as_ref()
                .and_then(|p| p.get("do_intrinsic").and_then(|v| v.as_bool()))
                .unwrap_or(false);
            let which = CameraIdentifier {
                group_name,
                camera_name,
            };
            let cal_req = CalibrationRequest {
                which_camera: Some(which),
                command: Some(CalibrateCommand {
                    do_extrinsic,
                    do_intrinsic,
                }),
            };
            let r = hub
                .calibrate(Request::new(cal_req))
                .await
                .map(|r| calibration_response_to_json(r.into_inner()));
            r
        }
        "Ping" => {
            let which = req.params.as_ref().map(|p| {
                let (group_name, camera_name) = (
                    p.get("group_name").and_then(|v| v.as_str()).map(String::from).unwrap_or_default(),
                    p.get("camera_name").and_then(|v| v.as_str()).map(String::from).unwrap_or_default(),
                );
                CameraIdentifier {
                    group_name,
                    camera_name,
                }
            });
            let timeout_secs = req
                .params
                .as_ref()
                .and_then(|p| p.get("timeout_secs").and_then(|v| v.as_u64()));
            let ping_req = PingRequest {
                which_camera: which,
                timeout: timeout_secs.map(|s| prost_types::Duration {
                    seconds: s as i64,
                    nanos: 0,
                }),
            };
            let r = hub
                .ping(Request::new(ping_req))
                .await
                .map(|r| ping_response_to_json(r.into_inner()));
            r
        }
        "UpdateCameras" => {
            let (group_name, camera_name) = parse_camera_identifier(&req.params);
            let which = CameraIdentifier {
                group_name,
                camera_name,
            };
            let update_req = UpdateCamerasRequest {
                which_camera: Some(which),
            };
            let r = hub
                .update_cameras(Request::new(update_req))
                .await
                .map(|r| {
                    let inner = r.into_inner();
                    serde_json::json!({ "cameras_updated": inner.cameras_updated })
                });
            r
        }
        _ => Err(tonic::Status::invalid_argument(format!("Unknown method: {}", req.method))),
    };
    match result {
        Ok(val) => serde_json::json!({ "id": id, "result": val }).to_string(),
        Err(e) => serde_json::json!({ "id": id, "error": e.message().to_string() }).to_string(),
    }
}

fn parse_camera_identifier(params: &Option<serde_json::Value>) -> (String, String) {
    let p = match params {
        Some(p) => p,
        None => return (String::new(), String::new()),
    };
    let group_name = p
        .get("group_name")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_default();
    let camera_name = p
        .get("camera_name")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_default();
    (group_name, camera_name)
}

fn stream_status_to_json(s: StreamStatus) -> serde_json::Value {
    serde_json::json!({
        "is_streaming": s.is_streaming,
        "params": s.params.map(|p| serde_json::json!({
            "fps": p.fps,
            "common": p.common.map(|c| serde_json::json!({
                "with_pose": c.with_pose,
                "with_image": c.with_image
            }))
        }))
    })
}

fn camera_info_to_json(info: CameraInfo) -> serde_json::Value {
    serde_json::json!({
        "which_camera": info.which_camera.map(|w| serde_json::json!({
            "group_name": w.group_name,
            "camera_name": w.camera_name
        })),
        "calibration": info.calibration.as_ref().map(|c| serde_json::json!({
            "intrinsics": c.intrinsics.as_ref().map(|i| serde_json::json!({
                "camera_matrix": i.camera_matrix,
                "distortion": i.distortion,
                "rms_error": i.rms_error
            })),
            "extrinsics": c.extrinsics.as_ref().map(|e| serde_json::json!({
                "view_matrix": e.view_matrix
            }))
        }))
    })
}

fn server_snapshot_response_to_json(r: ServerSnapshotResponse) -> serde_json::Value {
    let poses3d: Vec<serde_json::Value> = r
        .poses3d
        .iter()
        .filter_map(|p| serde_json::to_value(p).ok())
        .collect();
    serde_json::json!({
        "snapshot_id": r.snapshot_id,
        "timestamp": r.timestamp.map(|t| serde_json::json!({
            "seconds": t.seconds,
            "nanos": t.nanos
        })),
        "snapshots": r.snapshots.len(),
        "poses3d": poses3d
    })
}

fn calibration_response_to_json(r: CalibrationResponse) -> serde_json::Value {
    serde_json::json!({
        "states": r.states.iter().map(|s| serde_json::json!({
            "which_camera": s.which_camera.as_ref().map(|w| serde_json::json!({
                "group_name": w.group_name,
                "camera_name": w.camera_name
            })),
            "calibration": s.calibration.is_some()
        })).collect::<Vec<_>>()
    })
}

fn ping_response_to_json(r: PingResponse) -> serde_json::Value {
    serde_json::json!({
        "results": r.results.iter().map(|res| serde_json::json!({
            "which_camera": res.which_camera.as_ref().map(|w| serde_json::json!({
                "group_name": w.group_name,
                "camera_name": w.camera_name
            })),
            "response_time": res.response_time.as_ref().map(|d| serde_json::json!({
                "seconds": d.seconds,
                "nanos": d.nanos
            }))
        })).collect::<Vec<_>>()
    })
}

/// WebSocket server configuration.
pub struct WebSocketConfig {
    addr: SocketAddr,
}

impl WebSocketConfig {
    pub fn new(ip: &str, port: u16) -> anyhow::Result<Self> {
        Ok(Self {
            addr: format!("{}:{}", ip, port).parse()?,
        })
    }

    pub fn try_default() -> anyhow::Result<Self> {
        Self::new("0.0.0.0", 9001)
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self::try_default().expect("default WebSocket config 0.0.0.0:9001 must be valid")
    }
}

/// Type for a sink that sends WebSocket text messages.
type WsSender = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    Message,
>;

/// JSON-RPC style admin request over WebSocket.
#[derive(Debug, serde::Deserialize)]
struct WsAdminRequest {
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug)]
struct Client {
    tx: mpsc::UnboundedSender<Message>,
}

pub struct WebSocketServer {
    config: WebSocketConfig,
    stream_rx: broadcast::Receiver<PoseStreamUpdate>,
    admin: Option<Arc<HubServer>>,
}

impl WebSocketServer {
    pub fn new(
        config: WebSocketConfig,
        stream_rx: broadcast::Receiver<PoseStreamUpdate>,
        admin: Option<Arc<HubServer>>,
    ) -> Self {
        Self {
            config,
            stream_rx,
            admin,
        }
    }

    /// Run the server using the given listener (for tests; use port 0 to get a random port).
    pub async fn run_with_listener(self, listener: TcpListener) -> anyhow::Result<()> {
        log::info!(
            "PoseNet Hub WebSocket pose stream listening on ws://{}",
            listener.local_addr()?
        );
        self.run_accept_loop(listener).await
    }

    pub async fn run(self) -> anyhow::Result<()> {
        log::info!(
            "PoseNet Hub WebSocket pose stream listening on ws://{}",
            self.config.addr
        );
        let listener = TcpListener::bind(self.config.addr).await?;
        self.run_accept_loop(listener).await
    }

    async fn run_accept_loop(self, listener: TcpListener) -> anyhow::Result<()> {
        let clients: Arc<RwLock<Vec<Client>>> = Arc::new(RwLock::new(Vec::new()));
        let cameras_cache: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));
        let admin = self.admin.clone();

        let clients_for_accept = clients.clone();
        let cameras_cache_for_accept = cameras_cache.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                let (stream, _peer) = match listener.accept().await {
                    Ok(ok) => ok,
                    Err(e) => {
                        log::warn!("WebSocket accept error: {}", e);
                        continue;
                    }
                };
                let clients_ref = clients_for_accept.clone();
                let cameras_cache_ref = cameras_cache_for_accept.clone();
                let admin_for_conn = admin.clone();
                tokio::spawn(async move {
                    if let Ok(ws_stream) =
                        tokio_tungstenite::accept_async(stream).await
                    {
                        let (write_half, mut read_half) = ws_stream.split();
                        let (client_tx, mut client_rx) = mpsc::unbounded_channel::<Message>();
                        {
                            let mut guard = clients_ref.write().await;
                            guard.push(Client { tx: client_tx.clone() });
                        }
                        // Writer task: forward channel messages to the WebSocket.
                        let mut write_half = write_half;
                        tokio::spawn(async move {
                            while let Some(msg) = client_rx.recv().await {
                                if write_half.send(msg).await.is_err() {
                                    break;
                                }
                            }
                        });
                        // Send latest known cameras immediately on connect (if any).
                        if let Some(json) = cameras_cache_ref.read().await.clone() {
                            let _ = client_tx.send(Message::Text(json));
                        }
                        // Read loop: handle admin commands.
                        while let Some(ws_msg) = read_half.next().await {
                            let text = match ws_msg {
                                Ok(Message::Text(t)) => t,
                                _ => continue,
                            };
                            if let Ok(admin_req) = serde_json::from_str::<WsAdminRequest>(&text) {
                                if let Some(ref hub) = admin_for_conn {
                                    let response =
                                        handle_ws_admin(hub.as_ref(), admin_req).await;
                                    let _ = client_tx.send(Message::Text(response));
                                }
                            }
                        }
                    }
                });
            }
        });

        let mut stream_rx = self.stream_rx;
        let clients_for_broadcast = clients.clone();
        let cameras_cache_for_broadcast = cameras_cache.clone();
        let broadcast_handle = tokio::spawn(async move {
            while let Ok(update) = stream_rx.recv().await {
                let base_msg = stream_update_to_message(&update);

                // Camera updates: cache and broadcast (these happen on camera connect / optional updates).
                if !base_msg.cameras.is_empty() {
                    if let Ok(json) = serde_json::to_string(&base_msg) {
                        *cameras_cache_for_broadcast.write().await = Some(json.clone());
                        let mut guard = clients_for_broadcast.write().await;
                        let mut i = 0;
                        while i < guard.len() {
                            if guard[i].tx.send(Message::Text(json.clone())).is_err() {
                                log::debug!("WebSocket send error (client dropped?)");
                                let _ = guard.remove(i);
                            } else {
                                i += 1;
                            }
                        }
                    }
                    continue;
                }

                let n_poses = base_msg.poses.len();
                let mut guard = clients_for_broadcast.write().await;
                log::debug!(
                    "WebSocket: broadcasting group='{}' {} pose(s) to {} client(s)",
                    base_msg.group_name,
                    n_poses,
                    guard.len()
                );
                let mut i = 0;
                while i < guard.len() {
                    let json = match serde_json::to_string(&base_msg) {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!("WebSocket serialize error: {}", e);
                            i += 1;
                            continue;
                        }
                    };
                    if guard[i].tx.send(Message::Text(json)).is_err() {
                        log::debug!("WebSocket send error (client dropped?)");
                        let _ = guard.remove(i);
                    } else {
                        i += 1;
                    }
                }
            }
        });

        tokio::select! {
            _ = accept_handle => {}
            _ = broadcast_handle => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::Point2D;
    use crate::grpc::proto::Point3D;
    use crate::triangulator::LabeledPoses3D;
    use std::time::Instant;

    fn make_point3d(x: f64, y: f64, z: f64, score: f64) -> Point3D {
        Point3D { x, y, z, score }
    }

    fn make_point2d(x: f64, y: f64, score: f64) -> Point2D {
        Point2D { x, y, score }
    }

    #[test]
    fn test_point3d_to_json() {
        let p = make_point3d(1.0, 2.0, 3.0, 0.9);
        let j = point3d_to_json(&p);
        assert_eq!(j.x, 1.0);
        assert_eq!(j.y, 2.0);
        assert_eq!(j.z, 3.0);
        assert_eq!(j.score, 0.9);
    }

    #[test]
    fn test_pose3d_to_json_serializes() {
        let pose = Pose3D {
            nose: Some(make_point3d(0.0, 0.0, 0.0, 1.0)),
            left_eye: Some(make_point3d(1.0, 0.0, 0.0, 1.0)),
            right_eye: None,
            left_ear: None,
            right_ear: None,
            left_shoulder: None,
            right_shoulder: None,
            left_elbow: None,
            right_elbow: None,
            left_wrist: None,
            right_wrist: None,
            left_hip: None,
            right_hip: None,
            left_knee: None,
            right_knee: None,
            left_ankle: None,
            right_ankle: None,
            score: 0.95,
        };
        let j = pose3d_to_json(&pose);
        assert_eq!(j.keypoints.len(), 17);
        assert!(j.keypoints[0].is_some());
        assert!(j.keypoints[1].is_some());
        assert!(j.keypoints[2].is_none());
        assert_eq!(j.score, 0.95);
        let serialized = serde_json::to_string(&j).unwrap();
        assert!(serialized.contains("\"score\":0.95"));
    }

    #[test]
    fn test_point2d_to_json() {
        let p = make_point2d(10.0, 20.0, 0.8);
        let j = point2d_to_json(&p);
        assert_eq!(j.x, 10.0);
        assert_eq!(j.y, 20.0);
        assert_eq!(j.score, 0.8);
    }

    #[test]
    fn test_pose2d_to_json_serializes() {
        let pose = Pose2D {
            nose: Some(make_point2d(0.0, 0.0, 1.0)),
            left_eye: Some(make_point2d(1.0, 0.0, 1.0)),
            right_eye: None,
            left_ear: None,
            right_ear: None,
            left_shoulder: None,
            right_shoulder: None,
            left_elbow: None,
            right_elbow: None,
            left_wrist: None,
            right_wrist: None,
            left_hip: None,
            right_hip: None,
            left_knee: None,
            right_knee: None,
            left_ankle: None,
            right_ankle: None,
            score: 0.9,
        };
        let j = pose2d_to_json(&pose);
        assert_eq!(j.keypoints.len(), 17);
        assert!(j.keypoints[0].is_some());
        assert!(j.keypoints[1].is_some());
        assert_eq!(j.score, 0.9);
        let serialized = serde_json::to_string(&j).unwrap();
        assert!(serialized.contains("\"score\""));
    }

    #[test]
    fn test_pose_stream_message_serializes() {
        let update = PoseStreamUpdate {
            labeled_poses: LabeledPoses3D {
                group_name: "test_group".to_string(),
                poses: vec![],
                time: Instant::now(),
            },
            camera_views: vec![
                CameraView {
                    camera_name: "cam1".to_string(),
                    poses: vec![],
                },
                CameraView {
                    camera_name: "cam2".to_string(),
                    poses: vec![],
                },
            ],
            cameras: vec![],
        };
        let msg = stream_update_to_message(&update);
        assert_eq!(msg.group_name, "test_group");
        assert!(msg.poses.is_empty());
        assert_eq!(msg.camera_views.len(), 2);
        assert_eq!(msg.cameras.len(), 0);
        assert_eq!(msg.camera_views[0].camera_name, "cam1");
        assert_eq!(msg.camera_views[1].camera_name, "cam2");
        assert!(msg.timestamp_ms > 0);
        let serialized = serde_json::to_string(&msg).unwrap();
        assert!(serialized.contains("test_group"));
        assert!(serialized.contains("cam1"));
        assert!(serialized.contains("cam2"));
    }

    // --- Admin / WS JSON helpers (no HubServer required) ---

    #[test]
    fn test_parse_camera_identifier_none() {
        let (g, c) = parse_camera_identifier(&None);
        assert_eq!(g, "");
        assert_eq!(c, "");
    }

    #[test]
    fn test_parse_camera_identifier_empty_object() {
        let params = Some(serde_json::json!({}));
        let (g, c) = parse_camera_identifier(&params);
        assert_eq!(g, "");
        assert_eq!(c, "");
    }

    #[test]
    fn test_parse_camera_identifier_full() {
        let params = Some(serde_json::json!({
            "group_name": "my_group",
            "camera_name": "cam_0"
        }));
        let (g, c) = parse_camera_identifier(&params);
        assert_eq!(g, "my_group");
        assert_eq!(c, "cam_0");
    }

    #[test]
    fn test_parse_camera_identifier_group_only() {
        let params = Some(serde_json::json!({ "group_name": "g1" }));
        let (g, c) = parse_camera_identifier(&params);
        assert_eq!(g, "g1");
        assert_eq!(c, "");
    }

    #[test]
    fn test_stream_status_to_json() {
        use crate::grpc::proto::{SnapshotPayloadParameters, StreamParameters};
        let s = StreamStatus {
            is_streaming: true,
            params: Some(StreamParameters {
                common: Some(SnapshotPayloadParameters {
                    with_pose: true,
                    with_image: false,
                }),
                fps: 15.0,
            }),
        };
        let j = stream_status_to_json(s);
        assert_eq!(j.get("is_streaming"), Some(&serde_json::json!(true)));
        let params = j.get("params").and_then(|p| p.as_object()).unwrap();
        assert_eq!(params.get("fps"), Some(&serde_json::json!(15.0)));
    }

    #[test]
    fn test_stream_status_to_json_not_streaming() {
        let s = StreamStatus {
            is_streaming: false,
            params: None,
        };
        let j = stream_status_to_json(s);
        assert_eq!(j.get("is_streaming"), Some(&serde_json::json!(false)));
        assert!(j.get("params").unwrap().is_null());
    }

    #[test]
    fn test_ws_admin_request_deserialize() {
        let raw = r#"{"id":1,"method":"ListGroups","params":{}}"#;
        let req: WsAdminRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.method, "ListGroups");
        assert!(req.params.is_some());
    }

    #[test]
    fn test_ws_admin_request_deserialize_list_cameras() {
        let raw = r#"{"id":2,"method":"ListCameras","params":{"group_name":"g1"}}"#;
        let req: WsAdminRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.method, "ListCameras");
        let group = req.params.as_ref().and_then(|p| p.get("group_name")).and_then(|v| v.as_str()).unwrap();
        assert_eq!(group, "g1");
    }
}
