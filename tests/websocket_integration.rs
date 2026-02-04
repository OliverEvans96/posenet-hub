//! Integration tests for the WebSocket pose stream server.
//!
//! Connects a client, sends a pose update via the broadcast channel,
//! and asserts the client receives the expected JSON message.

use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use futures_util::StreamExt;

use posenet_vr_hub::grpc::proto::{Point3D, Pose3D};
use posenet_vr_hub::triangulator::{CameraView, PoseStreamUpdate};
use posenet_vr_hub::websocket::{WebSocketConfig, WebSocketServer};

fn make_point(x: f64, y: f64, z: f64, score: f64) -> Point3D {
    Point3D { x, y, z, score }
}

fn minimal_pose3d() -> Pose3D {
    Pose3D {
        nose: Some(make_point(0.0, 0.0, 0.0, 1.0)),
        left_eye: Some(make_point(0.1, 0.0, 0.0, 1.0)),
        right_eye: Some(make_point(-0.1, 0.0, 0.0, 1.0)),
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
    }
}

#[tokio::test]
async fn test_websocket_client_receives_pose_message() {
    let (tx, rx) = broadcast::channel::<PoseStreamUpdate>(10);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let config = WebSocketConfig::new("127.0.0.1", port).unwrap();
    let server = WebSocketServer::new(config, rx);

    let server_handle = tokio::spawn(async move {
        let _ = server.run_with_listener(listener).await;
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let ws_url = format!("ws://127.0.0.1:{}", port);
    let (mut ws_stream, _) = connect_async(ws_url).await.unwrap();

    let update = PoseStreamUpdate {
        labeled_poses: posenet_vr_hub::triangulator::LabeledPoses3D {
            group_name: "integration_test_group".to_string(),
            poses: vec![minimal_pose3d()],
            time: Instant::now(),
        },
        camera_views: vec![CameraView {
            camera_name: "test_cam".to_string(),
            poses: vec![],
        }],
        cameras: vec![],
    };
    let _ = tx.send(update);

    let msg = ws_stream.next().await.expect("expected one message");
    let msg = msg.expect("WebSocket message error");
    let text = msg.to_text().expect("expected text message");
    let parsed: serde_json::Value = serde_json::from_str(text).expect("valid JSON");
    assert_eq!(
        parsed["group_name"].as_str().unwrap(),
        "integration_test_group"
    );
    assert!(parsed["poses"].is_array());
    assert_eq!(parsed["poses"].as_array().unwrap().len(), 1);
    assert!(parsed["camera_views"].is_array());
    assert_eq!(parsed["camera_views"].as_array().unwrap().len(), 1);
    assert_eq!(
        parsed["camera_views"][0]["camera_name"].as_str().unwrap(),
        "test_cam"
    );
    assert!(parsed["timestamp_ms"].as_u64().is_some());

    server_handle.abort();
}
