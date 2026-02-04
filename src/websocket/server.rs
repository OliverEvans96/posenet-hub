//! WebSocket server that forwards pose broadcast messages to connected clients as JSON.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};

use crate::grpc::proto::{Point3D, Pose3D};
use crate::triangulator::LabeledPoses3D;

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

/// Message sent over WebSocket for each pose update.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PoseStreamMessage {
    pub group_name: String,
    pub poses: Vec<Pose3DJson>,
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

fn labeled_poses_to_message(labeled: &LabeledPoses3D) -> PoseStreamMessage {
    PoseStreamMessage {
        group_name: labeled.group_name.clone(),
        poses: labeled.poses.iter().map(pose3d_to_json).collect(),
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

/// Type for a sink that sends WebSocket text messages. Used to broadcast to clients.
type WsSender = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    Message,
>;

pub struct WebSocketServer {
    config: WebSocketConfig,
    poses3d_rx: broadcast::Receiver<LabeledPoses3D>,
}

impl WebSocketServer {
    pub fn new(
        config: WebSocketConfig,
        poses3d_rx: broadcast::Receiver<LabeledPoses3D>,
    ) -> Self {
        Self { config, poses3d_rx }
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
        let clients: Arc<RwLock<Vec<WsSender>>> = Arc::new(RwLock::new(Vec::new()));

        let clients_for_accept = clients.clone();
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
                tokio::spawn(async move {
                    if let Ok(ws_stream) =
                        tokio_tungstenite::accept_async(stream).await
                    {
                        let (write_half, mut read_half) = ws_stream.split();
                        {
                            let mut guard = clients_ref.write().await;
                            guard.push(write_half);
                        }
                        while read_half.next().await.is_some() {
                            // Drain incoming messages (we don't process client messages)
                        }
                    }
                });
            }
        });

        let mut poses3d_rx = self.poses3d_rx;
        let clients_for_broadcast = clients.clone();
        let broadcast_handle = tokio::spawn(async move {
            while let Ok(labeled) = poses3d_rx.recv().await {
                let msg = labeled_poses_to_message(&labeled);
                let n_poses = msg.poses.len();
                let json = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("WebSocket serialize error: {}", e);
                        continue;
                    }
                };
                let mut guard = clients_for_broadcast.write().await;
                log::debug!(
                    "WebSocket: broadcasting group='{}' {} pose(s) to {} client(s)",
                    msg.group_name,
                    n_poses,
                    guard.len()
                );
                let mut i = 0;
                while i < guard.len() {
                    let sender = &mut guard[i];
                    if let Err(e) = sender.send(Message::Text(json.clone())).await {
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
    use crate::grpc::proto::Point3D;

    fn make_point(x: f64, y: f64, z: f64, score: f64) -> Point3D {
        Point3D { x, y, z, score }
    }

    #[test]
    fn test_point3d_to_json() {
        let p = make_point(1.0, 2.0, 3.0, 0.9);
        let j = point3d_to_json(&p);
        assert_eq!(j.x, 1.0);
        assert_eq!(j.y, 2.0);
        assert_eq!(j.z, 3.0);
        assert_eq!(j.score, 0.9);
    }

    #[test]
    fn test_pose3d_to_json_serializes() {
        let pose = Pose3D {
            nose: Some(make_point(0.0, 0.0, 0.0, 1.0)),
            left_eye: Some(make_point(1.0, 0.0, 0.0, 1.0)),
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
    fn test_pose_stream_message_serializes() {
        let labeled = LabeledPoses3D {
            group_name: "test_group".to_string(),
            poses: vec![],
            time: std::time::Instant::now(),
        };
        let msg = labeled_poses_to_message(&labeled);
        assert_eq!(msg.group_name, "test_group");
        assert!(msg.poses.is_empty());
        assert!(msg.timestamp_ms > 0);
        let serialized = serde_json::to_string(&msg).unwrap();
        assert!(serialized.contains("test_group"));
    }
}
