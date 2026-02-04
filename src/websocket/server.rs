//! WebSocket server that forwards pose broadcast messages to connected clients as JSON.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};

use crate::grpc::proto::{Point2D, Point3D, Pose2D, Pose3D};
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

#[derive(Debug)]
struct Client {
    sender: WsSender,
}

pub struct WebSocketServer {
    config: WebSocketConfig,
    stream_rx: broadcast::Receiver<PoseStreamUpdate>,
}

impl WebSocketServer {
    pub fn new(
        config: WebSocketConfig,
        stream_rx: broadcast::Receiver<PoseStreamUpdate>,
    ) -> Self {
        Self { config, stream_rx }
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
                tokio::spawn(async move {
                    if let Ok(ws_stream) =
                        tokio_tungstenite::accept_async(stream).await
                    {
                        let (mut write_half, mut read_half) = ws_stream.split();
                        // Send latest known cameras immediately on connect (if any).
                        if let Some(json) = cameras_cache_ref.read().await.clone() {
                            let _ = write_half.send(Message::Text(json)).await;
                        }
                        {
                            let mut guard = clients_ref.write().await;
                            guard.push(Client { sender: write_half });
                        }
                        while read_half.next().await.is_some() {
                            // Drain incoming messages (we don't process client messages)
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
                            if let Err(e) =
                                guard[i].sender.send(Message::Text(json.clone())).await
                            {
                                log::debug!("WebSocket send error (client dropped?): {}", e);
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
                    if let Err(e) = guard[i].sender.send(Message::Text(json)).await {
                        log::debug!("WebSocket send error (client dropped?): {}", e);
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
}
