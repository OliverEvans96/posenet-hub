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
                do_intrinsic: false,
                do_extrinsic: true,
                pattern_cols: 5,
                pattern_rows: 7,
                square_size_m: 0.0285,
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

#[tokio::test]
async fn grpc_update_cameras_returns_count() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let req = grpc_client::build_update_cameras_request(CameraIdentifier {
            group_name: "update_group".to_string(),
            camera_name: String::new(),
        });
        let resp = grpc_client::update_cameras(&mut client, req)
            .await
            .expect("update_cameras");
        assert!(resp.cameras_updated >= 0);
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

#[tokio::test]
async fn grpc_take_snapshots_poses3d_multi_pose() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut admin = connect_client(port).await;
        let group = "grpc_poses3d_multi_group";

        let cal = test_fake_calibration();
        let cam1_info = CameraInfo {
            which_camera: Some(CameraIdentifier {
                group_name: group.to_string(),
                camera_name: "cam1".to_string(),
            }),
            calibration: Some(cal.clone()),
        };
        let cam2_info = CameraInfo {
            which_camera: Some(CameraIdentifier {
                group_name: group.to_string(),
                camera_name: "cam2".to_string(),
            }),
            calibration: Some(cal),
        };

        let mut client1 = connect_client(port).await;
        let mut client2 = connect_client(port).await;
        let token1 = client1.hello(Request::new(cam1_info.clone())).await.expect("hello").into_inner();
        let token2 = client2.hello(Request::new(cam2_info.clone())).await.expect("hello").into_inner();

        let (cmd_tx1, mut cmd_rx1) = mpsc::channel(4);
        let (cmd_tx2, mut cmd_rx2) = mpsc::channel(4);
        let mut cc1 = connect_client(port).await;
        let mut cc2 = connect_client(port).await;
        tokio::spawn(async move {
            let mut s = cc1.camera_control(Request::new(token1)).await.expect("cc").into_inner();
            while let Ok(Some(cmd)) = s.message().await {
                let _ = cmd_tx1.send(cmd).await;
            }
        });
        tokio::spawn(async move {
            let mut s = cc2.camera_control(Request::new(token2)).await.expect("cc").into_inner();
            while let Ok(Some(cmd)) = s.message().await {
                let _ = cmd_tx2.send(cmd).await;
            }
        });

        let take_req = ServerSnapshotRequest {
            which_camera: Some(CameraIdentifier { group_name: group.to_string(), camera_name: String::new() }),
            params: Some(SnapshotParameters {
                common: Some(SnapshotPayloadParameters { with_pose: true, with_image: false }),
            }),
            want_pose3d: true,
        };

        let poses1 = vec![full_pose2d(100.0, 200.0, 0.9), full_pose2d(150.0, 220.0, 0.9)];
        let poses2 = vec![full_pose2d(105.0, 200.0, 0.9), full_pose2d(155.0, 220.0, 0.9)];
        let which1 = cam1_info.which_camera.clone();
        let which2 = cam2_info.which_camera.clone();

        let snapshot_fut1 = async move {
            let cmd = timeout(Duration::from_secs(3), cmd_rx1.recv()).await.expect("cmd timeout").expect("closed");
            let command_token = cmd.token.expect("token").clone();
            let snapshot = Snapshot {
                timestamp: Some(prost_types::Timestamp { seconds: 0, nanos: 0 }),
                which_camera: which1,
                poses: poses1,
                image: None,
            };
            let msg = CameraMessage {
                msg: Some(camera_message::Msg::Response(posenet_vr_hub::grpc::proto::CommandResponse {
                    response: Some(command_response::Response::Snapshot(snapshot)),
                })),
            };
            let token_msg = CameraMessage { msg: Some(camera_message::Msg::Token(command_token)) };
            let stream = tokio_stream::iter([token_msg, msg]);
            client1.camera_data_sink(Request::new(stream)).await.expect("data_sink")
        };
        let snapshot_fut2 = async move {
            let cmd = timeout(Duration::from_secs(3), cmd_rx2.recv()).await.expect("cmd timeout").expect("closed");
            let command_token = cmd.token.expect("token").clone();
            let snapshot = Snapshot {
                timestamp: Some(prost_types::Timestamp { seconds: 0, nanos: 0 }),
                which_camera: which2,
                poses: poses2,
                image: None,
            };
            let msg = CameraMessage {
                msg: Some(camera_message::Msg::Response(posenet_vr_hub::grpc::proto::CommandResponse {
                    response: Some(command_response::Response::Snapshot(snapshot)),
                })),
            };
            let token_msg = CameraMessage { msg: Some(camera_message::Msg::Token(command_token)) };
            let stream = tokio_stream::iter([token_msg, msg]);
            client2.camera_data_sink(Request::new(stream)).await.expect("data_sink")
        };

        let (admin_resp, _, _) = tokio::join!(
            admin.take_snapshots(Request::new(take_req)),
            snapshot_fut1,
            snapshot_fut2,
        );

        let admin_resp = admin_resp.expect("take_snapshots").into_inner();
        assert!(!admin_resp.snapshot_id.is_empty());
        assert_eq!(admin_resp.snapshots.len(), 2);
        assert_eq!(admin_resp.snapshots[0].poses.len(), 2);
        assert_eq!(admin_resp.snapshots[1].poses.len(), 2);
        assert_eq!(admin_resp.poses3d.len(), 2, "want_pose3d with two cameras and two poses per camera => two 3D poses");
        assert!(admin_resp.poses3d[0].nose.is_some());
        assert!(admin_resp.poses3d[1].nose.is_some());
    })
    .await
    .expect("test timeout");
}

/// TakeSnapshots with 3 cameras and non-index-aligned poses (cam2 has swapped order).
/// Pose matching should recover correct correspondence and return 2 valid 3D poses.
#[tokio::test]
async fn grpc_take_snapshots_poses3d_three_cams_swapped_order() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut admin = connect_client(port).await;
        let group = "grpc_poses3d_three_swapped_group";

        let view0 = vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let view1 = vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let view2 = vec![1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let cam_infos = [
            CameraInfo {
                which_camera: Some(CameraIdentifier { group_name: group.to_string(), camera_name: "c0".to_string() }),
                calibration: Some(CalibrationParameters {
                    intrinsics: Some(CameraIntrinsics {
                        camera_matrix: vec![500.0, 0.0, 320.0, 0.0, 500.0, 240.0, 0.0, 0.0, 1.0],
                        distortion: vec![0.0; 5],
                        rms_error: 0.0,
                    }),
                    extrinsics: Some(CameraExtrinsics { view_matrix: view0 }),
                }),
            },
            CameraInfo {
                which_camera: Some(CameraIdentifier { group_name: group.to_string(), camera_name: "c1".to_string() }),
                calibration: Some(CalibrationParameters {
                    intrinsics: Some(CameraIntrinsics {
                        camera_matrix: vec![500.0, 0.0, 320.0, 0.0, 500.0, 240.0, 0.0, 0.0, 1.0],
                        distortion: vec![0.0; 5],
                        rms_error: 0.0,
                    }),
                    extrinsics: Some(CameraExtrinsics { view_matrix: view1 }),
                }),
            },
            CameraInfo {
                which_camera: Some(CameraIdentifier { group_name: group.to_string(), camera_name: "c2".to_string() }),
                calibration: Some(CalibrationParameters {
                    intrinsics: Some(CameraIntrinsics {
                        camera_matrix: vec![500.0, 0.0, 320.0, 0.0, 500.0, 240.0, 0.0, 0.0, 1.0],
                        distortion: vec![0.0; 5],
                        rms_error: 0.0,
                    }),
                    extrinsics: Some(CameraExtrinsics { view_matrix: view2 }),
                }),
            },
        ];

        let mut client0 = connect_client(port).await;
        let mut client1 = connect_client(port).await;
        let mut client2 = connect_client(port).await;
        let token0 = client0.hello(Request::new(cam_infos[0].clone())).await.expect("hello").into_inner();
        let token1 = client1.hello(Request::new(cam_infos[1].clone())).await.expect("hello").into_inner();
        let token2 = client2.hello(Request::new(cam_infos[2].clone())).await.expect("hello").into_inner();

        let (cmd_tx0, mut cmd_rx0) = mpsc::channel(4);
        let (cmd_tx1, mut cmd_rx1) = mpsc::channel(4);
        let (cmd_tx2, mut cmd_rx2) = mpsc::channel(4);
        let mut cc0 = connect_client(port).await;
        let mut cc1 = connect_client(port).await;
        let mut cc2 = connect_client(port).await;
        tokio::spawn(async move {
            let mut s = cc0.camera_control(Request::new(token0)).await.expect("cc").into_inner();
            while let Ok(Some(cmd)) = s.message().await { let _ = cmd_tx0.send(cmd).await; }
        });
        tokio::spawn(async move {
            let mut s = cc1.camera_control(Request::new(token1)).await.expect("cc").into_inner();
            while let Ok(Some(cmd)) = s.message().await { let _ = cmd_tx1.send(cmd).await; }
        });
        tokio::spawn(async move {
            let mut s = cc2.camera_control(Request::new(token2)).await.expect("cc").into_inner();
            while let Ok(Some(cmd)) = s.message().await { let _ = cmd_tx2.send(cmd).await; }
        });

        let take_req = ServerSnapshotRequest {
            which_camera: Some(CameraIdentifier { group_name: group.to_string(), camera_name: String::new() }),
            params: Some(SnapshotParameters {
                common: Some(SnapshotPayloadParameters { with_pose: true, with_image: false }),
            }),
            want_pose3d: true,
        };

        // Subject A at ~(320,240), subject B at ~(100,100). Cam0 and cam1: [A, B]; cam2: [B, A] (swapped).
        let pose_a = full_pose2d(320.0, 240.0, 0.9);
        let pose_b = full_pose2d(100.0, 100.0, 0.9);
        let poses_cam0 = vec![pose_a.clone(), pose_b.clone()];
        let poses_cam1 = vec![pose_a.clone(), pose_b.clone()];
        let poses_cam2_swapped = vec![pose_b.clone(), pose_a.clone()];
        let which0 = cam_infos[0].which_camera.clone();
        let which1 = cam_infos[1].which_camera.clone();
        let which2 = cam_infos[2].which_camera.clone();

        let snapshot_fut0 = async move {
            let cmd = timeout(Duration::from_secs(3), cmd_rx0.recv()).await.expect("cmd timeout").expect("closed");
            let token = cmd.token.expect("token").clone();
            let snapshot = Snapshot {
                timestamp: Some(prost_types::Timestamp { seconds: 0, nanos: 0 }),
                which_camera: which0,
                poses: poses_cam0,
                image: None,
            };
            let msg = CameraMessage {
                msg: Some(camera_message::Msg::Response(posenet_vr_hub::grpc::proto::CommandResponse {
                    response: Some(command_response::Response::Snapshot(snapshot)),
                })),
            };
            let stream = tokio_stream::iter([CameraMessage { msg: Some(camera_message::Msg::Token(token)) }, msg]);
            client0.camera_data_sink(Request::new(stream)).await.expect("data_sink")
        };
        let snapshot_fut1 = async move {
            let cmd = timeout(Duration::from_secs(3), cmd_rx1.recv()).await.expect("cmd timeout").expect("closed");
            let token = cmd.token.expect("token").clone();
            let snapshot = Snapshot {
                timestamp: Some(prost_types::Timestamp { seconds: 0, nanos: 0 }),
                which_camera: which1,
                poses: poses_cam1,
                image: None,
            };
            let msg = CameraMessage {
                msg: Some(camera_message::Msg::Response(posenet_vr_hub::grpc::proto::CommandResponse {
                    response: Some(command_response::Response::Snapshot(snapshot)),
                })),
            };
            let stream = tokio_stream::iter([CameraMessage { msg: Some(camera_message::Msg::Token(token)) }, msg]);
            client1.camera_data_sink(Request::new(stream)).await.expect("data_sink")
        };
        let snapshot_fut2 = async move {
            let cmd = timeout(Duration::from_secs(3), cmd_rx2.recv()).await.expect("cmd timeout").expect("closed");
            let token = cmd.token.expect("token").clone();
            let snapshot = Snapshot {
                timestamp: Some(prost_types::Timestamp { seconds: 0, nanos: 0 }),
                which_camera: which2,
                poses: poses_cam2_swapped,
                image: None,
            };
            let msg = CameraMessage {
                msg: Some(camera_message::Msg::Response(posenet_vr_hub::grpc::proto::CommandResponse {
                    response: Some(command_response::Response::Snapshot(snapshot)),
                })),
            };
            let stream = tokio_stream::iter([CameraMessage { msg: Some(camera_message::Msg::Token(token)) }, msg]);
            client2.camera_data_sink(Request::new(stream)).await.expect("data_sink")
        };

        let (admin_resp, _, _, _) = tokio::join!(
            admin.take_snapshots(Request::new(take_req)),
            snapshot_fut0,
            snapshot_fut1,
            snapshot_fut2,
        );

        let admin_resp = admin_resp.expect("take_snapshots").into_inner();
        assert!(!admin_resp.snapshot_id.is_empty());
        assert_eq!(admin_resp.snapshots.len(), 3);
        assert_eq!(admin_resp.poses3d.len(), 2,
            "pose matching with 3 cams and swapped order on cam2 should still yield 2 correct 3D poses");
        assert!(admin_resp.poses3d[0].nose.is_some());
        assert!(admin_resp.poses3d[1].nose.is_some());
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

#[tokio::test]
async fn grpc_triangulate_returns_multiple_poses() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;

        let cam1 = camera_info_for_triangulation(
            "tri_multi",
            "c1",
            vec![
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        );
        let cam2 = camera_info_for_triangulation(
            "tri_multi",
            "c2",
            vec![
                1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        );
        let pose1_c1 = full_pose2d(320.0, 240.0, 1.0);
        let pose2_c1 = full_pose2d(325.0, 245.0, 1.0);
        let pose1_c2 = full_pose2d(330.0, 240.0, 1.0);
        let pose2_c2 = full_pose2d(335.0, 245.0, 1.0);

        let req_stream = tokio_stream::iter([
            posenet_vr_hub::grpc::proto::TriangulationRequest {
                camera: Some(cam1),
                poses: vec![pose1_c1, pose2_c1],
            },
            posenet_vr_hub::grpc::proto::TriangulationRequest {
                camera: Some(cam2),
                poses: vec![pose1_c2, pose2_c2],
            },
        ]);

        let response = client
            .triangulate(Request::new(req_stream))
            .await
            .expect("triangulate")
            .into_inner();
        assert_eq!(response.poses.len(), 2, "expect two 3D poses (pose order consistent across cameras)");
        assert!(response.poses[0].nose.is_some());
        assert!(response.poses[1].nose.is_some());
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
