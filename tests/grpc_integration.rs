//! Comprehensive integration tests for the Hub gRPC API.
//!
//! Exercises all HubService RPCs against a real server:
//! - Camera: Hello, CameraControl (stream), CameraDataSink (stream)
//! - Admin: Ping, ListGroups, ListCameras, GetCameraInfo, StreamControl,
//!   Calibrate, TakeSnapshots, GetCurrent
//! - A la carte: Triangulate (stream), BundleAdjustment
//!
//! Uses a per-test timeout so the runner does not hang if the server stalls.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use tonic::transport::Channel;
use tonic::Request;

use posenet_vr_hub::grpc::client as grpc_client;
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;
use posenet_vr_hub::grpc::proto::camera_message;
use posenet_vr_hub::grpc::proto::command_response;
use posenet_vr_hub::grpc::proto::CameraInfo;
use posenet_vr_hub::grpc::proto::CameraMessage;
use posenet_vr_hub::grpc::proto::CalibrationParameters;
use posenet_vr_hub::grpc::proto::CalibrateCommand;
use posenet_vr_hub::grpc::proto::CalibrationRequest;
use posenet_vr_hub::grpc::proto::CameraExtrinsics;
use posenet_vr_hub::grpc::proto::CameraIdentifier;
use posenet_vr_hub::grpc::proto::CameraIntrinsics;
use posenet_vr_hub::grpc::proto::Point2D;
use posenet_vr_hub::grpc::proto::PingRequest;
use posenet_vr_hub::grpc::proto::Pose2D;
use posenet_vr_hub::grpc::proto::ServerSnapshotRequest;
use posenet_vr_hub::grpc::proto::Snapshot;
use posenet_vr_hub::grpc::proto::SnapshotParameters;
use posenet_vr_hub::grpc::proto::SnapshotPayloadParameters;

const TEST_PORT_BASE: u16 = 50100;
const SERVER_STARTUP_MS: u64 = 200;
const TEST_TIMEOUT: Duration = Duration::from_secs(15);
static NEXT_TEST_PORT: AtomicU16 = AtomicU16::new(TEST_PORT_BASE);

fn test_fake_calibration() -> CalibrationParameters {
    CalibrationParameters {
        intrinsics: Some(CameraIntrinsics {
            camera_matrix: vec![100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0],
            distortion: vec![0.0, 0.0, 0.0, 0.0, 0.0],
            rms_error: 0.0,
        }),
        extrinsics: Some(CameraExtrinsics {
            view_matrix: vec![
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }),
    }
}

fn camera_info_for_triangulation(
    group_name: &str,
    camera_name: &str,
    view_matrix: Vec<f64>,
) -> CameraInfo {
    CameraInfo {
        which_camera: Some(CameraIdentifier {
            group_name: group_name.to_string(),
            camera_name: camera_name.to_string(),
        }),
        calibration: Some(CalibrationParameters {
            intrinsics: Some(CameraIntrinsics {
                camera_matrix: vec![500.0, 0.0, 320.0, 0.0, 500.0, 240.0, 0.0, 0.0, 1.0],
                distortion: vec![0.0, 0.0, 0.0, 0.0, 0.0],
                rms_error: 0.0,
            }),
            extrinsics: Some(CameraExtrinsics { view_matrix }),
        }),
    }
}

/// Build a Pose2D with all 17 keypoints set (required by triangulation).
fn full_pose2d(x: f64, y: f64, score: f64) -> Pose2D {
    let pt = Point2D { x, y, score };
    Pose2D {
        nose: Some(pt.clone()),
        left_eye: Some(pt.clone()),
        right_eye: Some(pt.clone()),
        left_ear: Some(pt.clone()),
        right_ear: Some(pt.clone()),
        left_shoulder: Some(pt.clone()),
        right_shoulder: Some(pt.clone()),
        left_elbow: Some(pt.clone()),
        right_elbow: Some(pt.clone()),
        left_wrist: Some(pt.clone()),
        right_wrist: Some(pt.clone()),
        left_hip: Some(pt.clone()),
        right_hip: Some(pt.clone()),
        left_knee: Some(pt.clone()),
        right_knee: Some(pt.clone()),
        left_ankle: Some(pt.clone()),
        right_ankle: Some(pt),
        score,
    }
}

async fn start_test_server() -> u16 {
    let port = NEXT_TEST_PORT.fetch_add(1, Ordering::SeqCst);
    let (cameras_tx, cameras_rx) = mpsc::unbounded_channel();
    let (snapshots_tx, snapshots_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let _ = cameras_rx;
        let _ = snapshots_rx;
        std::future::pending::<()>().await
    });
    let hub = Arc::new(posenet_vr_hub::grpc::server::HubServer::new(cameras_tx, snapshots_tx));
    let config =
        posenet_vr_hub::grpc::server::GrpcConfig::new("127.0.0.1", port).expect("test GrpcConfig");
    let grpc_server = posenet_vr_hub::grpc::server::GrpcServer::new(config, hub);
    tokio::spawn(async move {
        let _ = grpc_server.run().await;
    });
    sleep(Duration::from_millis(SERVER_STARTUP_MS)).await;
    port
}

async fn connect_client(port: u16) -> HubServiceClient<Channel> {
    let addr = format!("http://127.0.0.1:{}", port);
    timeout(TEST_TIMEOUT, HubServiceClient::connect(addr))
        .await
        .expect("connect timeout")
        .expect("client connect")
}

// ---- Hello & Admin (consolidated) ----

#[tokio::test]
async fn grpc_hello_returns_session_token() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let group = "grpc_hello_group";
        let camera_name = grpc_client::hello(&mut client, group.to_string())
            .await
            .expect("hello");
        assert!(!camera_name.is_empty());
        let groups = grpc_client::list_groups(&mut client).await.expect("list_groups");
        assert_eq!(groups, vec![group]);
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn grpc_list_groups_list_cameras_get_camera_info() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        assert!(grpc_client::list_groups(&mut client).await.unwrap().is_empty());

        let group = "grpc_list_group";
        let camera_name = grpc_client::hello(&mut client, group.to_string())
            .await
            .expect("hello");

        let groups = grpc_client::list_groups(&mut client).await.expect("list_groups");
        assert_eq!(groups, vec![group]);

        let cameras = grpc_client::list_cameras(&mut client, group.to_string())
            .await
            .expect("list_cameras");
        assert_eq!(cameras, vec![camera_name.clone()]);

        let which = CameraIdentifier {
            group_name: group.to_string(),
            camera_name,
        };
        let info = grpc_client::get_camera_info(&mut client, which)
            .await
            .expect("get_camera_info");
        assert!(info.which_camera.is_some());
        assert!(info.calibration.is_some());
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn grpc_ping_with_and_without_camera() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;

        let resp = grpc_client::ping(
            &mut client,
            PingRequest {
                which_camera: None,
                timeout: None,
            },
        )
        .await
        .expect("ping");
        assert!(resp.results.is_empty());

        let _ = grpc_client::hello(&mut client, "ping_group".to_string())
            .await
            .expect("hello");
        let resp = grpc_client::ping(
            &mut client,
            PingRequest {
                which_camera: Some(CameraIdentifier {
                    group_name: "ping_group".to_string(),
                    camera_name: String::new(),
                }),
                timeout: None,
            },
        )
        .await
        .expect("ping");
        assert!(resp.results.len() <= 1);
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn grpc_stream_control_start_and_stop() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let group = "grpc_stream_group";

        let status = grpc_client::stream_control_start(&mut client, group.to_string(), true, false, None)
            .await
            .expect("stream_control start");
        assert!(status.is_streaming);
        assert!(status.params.is_some());

        let status = grpc_client::stream_control_stop(&mut client, group.to_string())
            .await
            .expect("stream_control stop");
        assert!(!status.is_streaming);
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn grpc_take_snapshots_and_get_current() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let group = "grpc_snap_group";

        let which = CameraIdentifier {
            group_name: group.to_string(),
            camera_name: String::new(),
        };
        let req = ServerSnapshotRequest {
            which_camera: Some(which.clone()),
            params: Some(SnapshotParameters {
                common: Some(SnapshotPayloadParameters {
                    with_pose: true,
                    with_image: false,
                }),
            }),
            want_pose3d: false,
        };
        let resp = grpc_client::take_snapshots(&mut client, req)
            .await
            .expect("take_snapshots");
        assert!(!resp.snapshot_id.is_empty());
        assert!(resp.snapshots.is_empty());

        let result = grpc_client::get_current(&mut client, which).await;
        assert!(result.is_err());
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn grpc_calibrate_returns_response() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let req = CalibrationRequest {
            which_camera: Some(CameraIdentifier {
                group_name: "cal_group".to_string(),
                camera_name: String::new(),
            }),
            command: Some(CalibrateCommand {
                do_extrinsic: true,
                do_intrinsic: false,
            }),
        };
        let resp = grpc_client::calibrate(&mut client, req)
            .await
            .expect("calibrate");
        assert!(resp.states.is_empty());
    })
    .await
    .expect("test timeout");
}

// ---- CameraControl stream: camera receives commands ----

#[tokio::test]
async fn grpc_camera_control_stream_receives_start_streaming_command() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let _admin = connect_client(port).await;
        let group = "grpc_ctrl_stream_group";

        let mut camera_client = connect_client(port).await;
        let camera_info = CameraInfo {
            which_camera: Some(CameraIdentifier {
                group_name: group.to_string(),
                camera_name: "cam1".to_string(),
            }),
            calibration: Some(test_fake_calibration()),
        };
        let session_token = camera_client
            .hello(Request::new(camera_info))
            .await
            .expect("camera hello")
            .into_inner();

        let mut stream = camera_client
            .camera_control(Request::new(session_token))
            .await
            .expect("camera_control")
            .into_inner();

        // Listen for the command in a task so we're polling the stream before sending.
        let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
        let listen_handle = tokio::spawn(async move {
            let msg = stream.message().await;
            let _ = cmd_tx.send(msg).await;
        });

        sleep(Duration::from_millis(100)).await;

        // Run stream_control_start in background: server sends the command immediately
        // but then blocks waiting for camera to respond via CameraDataSink (we don't do that here).
        let mut admin2 = connect_client(port).await;
        let group_clone = group.to_string();
        let _start_handle = tokio::spawn(async move {
            let _ = grpc_client::stream_control_start(&mut admin2, group_clone, true, false, None).await;
        });

        let first_cmd = timeout(
            Duration::from_secs(5),
            cmd_rx.recv(),
        )
        .await
        .expect("receive timeout")
        .expect("channel closed");
        let first_cmd = first_cmd.expect("stream ok").expect("some command");
        listen_handle.await.expect("listen task join");
        assert!(first_cmd.token.is_some());
        match first_cmd.command {
            Some(posenet_vr_hub::grpc::proto::camera_control_command::Command::StartStreaming(_)) => {}
            other => panic!("expected StartStreaming command, got {:?}", other),
        }
    })
    .await
    .expect("test timeout");
}

// ---- CameraDataSink: admin TakeSnapshots, camera responds via data sink ----

#[tokio::test]
async fn grpc_take_snapshots_with_camera_data_sink_flow() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut admin = connect_client(port).await;
        let group = "grpc_datasink_group";

        let camera_info = CameraInfo {
            which_camera: Some(CameraIdentifier {
                group_name: group.to_string(),
                camera_name: "cam1".to_string(),
            }),
            calibration: Some(test_fake_calibration()),
        };

        let mut camera_client = connect_client(port).await;
        let session_token = camera_client
            .hello(Request::new(camera_info.clone()))
            .await
            .expect("camera hello")
            .into_inner();

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<posenet_vr_hub::grpc::proto::CameraControlCommand>(4);
        let mut camera_client_cc = connect_client(port).await;
        let session_token_cc = session_token.clone();
        tokio::spawn(async move {
            let mut stream = camera_client_cc
                .camera_control(Request::new(session_token_cc))
                .await
                .expect("camera_control")
                .into_inner();
            while let Ok(Some(cmd)) = stream.message().await {
                let _ = cmd_tx.send(cmd).await;
            }
        });

        let which = CameraIdentifier {
            group_name: group.to_string(),
            camera_name: String::new(),
        };
        let take_req = ServerSnapshotRequest {
            which_camera: Some(which),
            params: Some(SnapshotParameters {
                common: Some(SnapshotPayloadParameters {
                    with_pose: true,
                    with_image: false,
                }),
            }),
            want_pose3d: false,
        };

        let snapshot_response = async {
            let cmd = timeout(Duration::from_secs(3), cmd_rx.recv())
                .await
                .expect("wait for command timeout")
                .expect("channel closed");
            let command_token = cmd.token.expect("command has token").clone();
            let snapshot = Snapshot {
                timestamp: Some(prost_types::Timestamp {
                    seconds: 0,
                    nanos: 0,
                }),
                which_camera: camera_info.which_camera.clone(),
                poses: vec![full_pose2d(100.0, 200.0, 0.9)],
                image: None,
            };
            let response = command_response::Response::Snapshot(snapshot);
            let msg = CameraMessage {
                msg: Some(camera_message::Msg::Response(posenet_vr_hub::grpc::proto::CommandResponse {
                    response: Some(response),
                })),
            };
            let token_msg = CameraMessage {
                msg: Some(camera_message::Msg::Token(command_token)),
            };
            let stream = tokio_stream::iter([token_msg, msg]);
            camera_client
                .camera_data_sink(Request::new(stream))
                .await
                .expect("camera_data_sink")
        };

        let (admin_resp, _sink_resp) = tokio::join!(
            admin.take_snapshots(Request::new(take_req)),
            snapshot_response,
        );

        let admin_resp = admin_resp.expect("take_snapshots").into_inner();
        assert!(!admin_resp.snapshot_id.is_empty());
        assert_eq!(admin_resp.snapshots.len(), 1);
        assert_eq!(
            admin_resp.snapshots[0].poses.len(),
            1,
            "expected one pose in snapshot"
        );
    })
    .await
    .expect("test timeout");
}

// ---- Triangulate (streaming RPC) ----

#[tokio::test]
async fn grpc_triangulate_returns_poses() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;

        let cam1 = camera_info_for_triangulation(
            "tri",
            "c1",
            vec![
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        );
        let cam2 = camera_info_for_triangulation(
            "tri",
            "c2",
            vec![
                1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        );
        let pose1 = full_pose2d(320.0, 240.0, 1.0);
        let pose2 = full_pose2d(330.0, 240.0, 1.0);

        let req_stream = tokio_stream::iter([
            posenet_vr_hub::grpc::proto::TriangulationRequest {
                camera: Some(cam1),
                poses: vec![pose1],
            },
            posenet_vr_hub::grpc::proto::TriangulationRequest {
                camera: Some(cam2),
                poses: vec![pose2],
            },
        ]);

        let response = client
            .triangulate(Request::new(req_stream))
            .await
            .expect("triangulate")
            .into_inner();
        assert_eq!(response.poses.len(), 1);
        let pose3d = &response.poses[0];
        assert!(pose3d.nose.is_some());
    })
    .await
    .expect("test timeout");
}

// ---- BundleAdjustment ----

#[tokio::test]
async fn grpc_bundle_adjustment_returns_cameras_and_poses() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;

        let cam1 = camera_info_for_triangulation(
            "ba",
            "c1",
            vec![
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        );
        let cam2 = camera_info_for_triangulation(
            "ba",
            "c2",
            vec![
                1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        );
        let pose1 = full_pose2d(320.0, 240.0, 1.0);
        let pose2 = full_pose2d(330.0, 240.0, 1.0);

        use posenet_vr_hub::grpc::proto::BundleAdjustmentRequest;
        use posenet_vr_hub::grpc::proto::BundleAdjustmentOptions;
        use posenet_vr_hub::grpc::proto::Point3D;

        let initial_pose = posenet_vr_hub::grpc::proto::Pose3D {
            nose: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            left_eye: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            right_eye: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            left_ear: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            right_ear: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            left_shoulder: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            right_shoulder: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            left_elbow: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            right_elbow: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            left_wrist: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            right_wrist: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            left_hip: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            right_hip: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            left_knee: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            right_knee: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            left_ankle: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            right_ankle: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 5.0,
                score: 1.0,
            }),
            score: 1.0,
        };

        let req = BundleAdjustmentRequest {
            views: vec![
                posenet_vr_hub::grpc::proto::TriangulationRequest {
                    camera: Some(cam1),
                    poses: vec![pose1],
                },
                posenet_vr_hub::grpc::proto::TriangulationRequest {
                    camera: Some(cam2),
                    poses: vec![pose2],
                },
            ],
            initial_poses: vec![initial_pose],
            options: Some(BundleAdjustmentOptions {
                camera_rotation: true,
                camera_translation: true,
                camera_intrinsics: false,
                pose3d: true,
            }),
        };

        let response = client
            .bundle_adjustment(Request::new(req))
            .await
            .expect("bundle_adjustment")
            .into_inner();
        assert_eq!(response.cameras.len(), 2);
        assert_eq!(response.poses.len(), 1);
    })
    .await
    .expect("test timeout");
}
