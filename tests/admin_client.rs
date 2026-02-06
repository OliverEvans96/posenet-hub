//! Integration tests for the Admin CLI client library.
//! Starts a real gRPC server, registers a camera via Hello, then exercises
//! all 8 Admin endpoint wrappers (list_groups, list_cameras, get_camera_info,
//! stream_control, take_snapshots, get_current, calibrate, ping).
//!
//! All integration tests use a per-test timeout (TEST_TIMEOUT) so the test
//! runner does not hang if the server or network stalls.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use tonic::transport::Channel;

use posenet_vr_hub::grpc::client as grpc_client;
use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;
use posenet_vr_hub::grpc::proto::{
    CalibrateCommand, CalibrationRequest, CameraIdentifier, PingRequest, ServerSnapshotRequest,
    SnapshotParameters, SnapshotPayloadParameters,
};

const TEST_PORT_BASE: u16 = 50052;
const SERVER_STARTUP_MS: u64 = 200;
/// Per-test timeout so tests don't hang forever (e.g. if server fails to bind or connect).
const TEST_TIMEOUT: Duration = Duration::from_secs(10);
static NEXT_TEST_PORT: AtomicU16 = AtomicU16::new(TEST_PORT_BASE);

async fn start_test_server() -> u16 {
    let port = NEXT_TEST_PORT.fetch_add(1, Ordering::SeqCst);
    let (cameras_tx, cameras_rx) = mpsc::unbounded_channel();
    let (snapshots_tx, snapshots_rx) = mpsc::unbounded_channel();
    // Keep receivers alive so server can send (Hello sends to cameras_tx; dropping rx closes the channel).
    tokio::spawn(async move {
        let _ = cameras_rx;
        let _ = snapshots_rx;
        std::future::pending::<()>().await
    });
    let hub = Arc::new(posenet_vr_hub::grpc::server::HubServer::new(
        cameras_tx,
        snapshots_tx,
        None,
    ));
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

async fn run_admin_client_list_groups_returns_registered_groups() {
    let port = start_test_server().await;
    let mut client = connect_client(port).await;

    // Empty before any camera registers
    let groups = timeout(TEST_TIMEOUT, grpc_client::list_groups(&mut client))
        .await
        .expect("list_groups timeout")
        .expect("list_groups");
    assert!(
        groups.is_empty(),
        "expected no groups initially, got {:?}",
        groups
    );

    // Register a camera
    let group_name = "test_group_list_groups".to_string();
    let _camera_name = timeout(
        TEST_TIMEOUT,
        grpc_client::hello(&mut client, group_name.clone()),
    )
    .await
    .expect("hello timeout")
    .expect("hello");

    let groups = timeout(TEST_TIMEOUT, grpc_client::list_groups(&mut client))
        .await
        .expect("list_groups timeout")
        .expect("list_groups");
    assert_eq!(
        groups,
        vec![group_name],
        "list_groups should return registered group"
    );
}

#[tokio::test]
async fn admin_client_list_groups_returns_registered_groups() {
    timeout(
        TEST_TIMEOUT,
        run_admin_client_list_groups_returns_registered_groups(),
    )
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_list_cameras_returns_cameras_in_group() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let group_name = "test_group_list_cameras".to_string();
        let camera_name = grpc_client::hello(&mut client, group_name.clone())
            .await
            .expect("hello");
        let cameras = grpc_client::list_cameras(&mut client, group_name.clone())
            .await
            .expect("list_cameras");
        assert_eq!(
            cameras,
            vec![camera_name],
            "list_cameras should return registered camera"
        );
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_list_cameras_empty_for_unknown_group() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let cameras = grpc_client::list_cameras(&mut client, "nonexistent_group".to_string())
            .await
            .expect("list_cameras");
        assert!(cameras.is_empty(), "unknown group should return empty list");
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_get_camera_info_returns_info_for_registered_camera() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let group_name = "test_group_get_info".to_string();
        let camera_name = grpc_client::hello(&mut client, group_name.clone())
            .await
            .expect("hello");
        let which_camera = CameraIdentifier {
            group_name: group_name.clone(),
            camera_name: camera_name.clone(),
        };
        let info = grpc_client::get_camera_info(&mut client, which_camera)
            .await
            .expect("get_camera_info");
        assert!(info.which_camera.is_some());
        let id = info.which_camera.as_ref().unwrap();
        assert_eq!(id.group_name, group_name);
        assert_eq!(id.camera_name, camera_name);
        assert!(info.calibration.is_some());
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_get_camera_info_fails_for_unknown_camera() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let which_camera = CameraIdentifier {
            group_name: "no_group".to_string(),
            camera_name: "no_camera".to_string(),
        };
        let result = grpc_client::get_camera_info(&mut client, which_camera).await;
        assert!(
            result.is_err(),
            "get_camera_info should fail for unknown camera"
        );
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_stream_control_start_returns_streaming_status() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        // Use a group with no registered cameras so server returns immediately (0 sessions → 0 execution futures).
        let group_name = "test_group_stream".to_string();
        let status = grpc_client::stream_control_start(&mut client, group_name, true, false, None)
            .await
            .expect("stream_control start");
        assert!(
            status.is_streaming,
            "start streaming should return is_streaming true"
        );
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_stream_control_stop_returns_not_streaming() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        // Use a group with no cameras so server returns immediately.
        let group_name = "test_group_stop".to_string();
        let status = grpc_client::stream_control_stop(&mut client, group_name)
            .await
            .expect("stream_control stop");
        assert!(
            !status.is_streaming,
            "stop streaming should return is_streaming false"
        );
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_take_snapshots_returns_response() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        // Use a group with no registered cameras so server returns immediately (empty snapshots).
        let group_name = "test_group_snap".to_string();
        let which_camera = CameraIdentifier {
            group_name: group_name.clone(),
            camera_name: String::new(),
        };
        let params = SnapshotParameters {
            common: Some(SnapshotPayloadParameters {
                with_pose: true,
                with_image: false,
            }),
        };
        let req = ServerSnapshotRequest {
            which_camera: Some(which_camera),
            params: Some(params),
            want_pose3d: false,
        };
        let resp = grpc_client::take_snapshots(&mut client, req)
            .await
            .expect("take_snapshots");
        assert!(!resp.snapshot_id.is_empty());
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_get_current_fails_when_no_cached_snapshot() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let which_camera = CameraIdentifier {
            group_name: "test_group".to_string(),
            camera_name: String::new(),
        };
        let result = grpc_client::get_current(&mut client, which_camera).await;
        assert!(
            result.is_err(),
            "get_current should fail when cache is empty"
        );
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_calibrate_returns_response() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        // Use a group with no registered cameras so server returns immediately (empty states).
        let group_name = "test_group_cal".to_string();
        let which_camera = CameraIdentifier {
            group_name,
            camera_name: String::new(),
        };
        let req = CalibrationRequest {
            which_camera: Some(which_camera),
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
        assert!(
            resp.states.is_empty(),
            "no cameras so states should be empty"
        );
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_ping_with_camera_returns_response() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let group_name = "test_group_ping".to_string();
        let _ = grpc_client::hello(&mut client, group_name.clone())
            .await
            .expect("hello");
        let which_camera = CameraIdentifier {
            group_name,
            camera_name: String::new(),
        };
        let req = PingRequest {
            which_camera: Some(which_camera),
            timeout: None,
        };
        let resp = grpc_client::ping(&mut client, req).await.expect("ping");
        assert!(resp.results.len() <= 1);
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn admin_client_ping_without_camera_returns_empty() {
    timeout(TEST_TIMEOUT, async {
        let port = start_test_server().await;
        let mut client = connect_client(port).await;
        let req = PingRequest {
            which_camera: None,
            timeout: None,
        };
        let resp = grpc_client::ping(&mut client, req).await.expect("ping");
        assert!(
            resp.results.is_empty(),
            "ping with no camera should return empty results"
        );
    })
    .await
    .expect("test timeout");
}
