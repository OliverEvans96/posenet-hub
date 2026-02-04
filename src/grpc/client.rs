use rand::distributions::Alphanumeric;
use rand::prelude::ThreadRng;
use rand::{thread_rng, Rng};
use std::time::SystemTime;
use tokio::sync::mpsc::{channel, Sender};
use tokio::time::{sleep, Duration};
use tonic::{transport::Channel, Request};

use super::proto::hub_service_client::HubServiceClient;
use super::proto::*;
use super::proto::stream_control_request;

fn fake_calibration() -> CalibrationParameters {
    let mut rng = thread_rng();
    let intrinsics = CameraIntrinsics {
        camera_matrix: vec![100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0],
        distortion: vec![0.0, 0.0, 0.0, 0.0, 0.0],
        rms_error: 0.0,
    };
    let extrinsics = CameraExtrinsics {
        view_matrix: vec![
            1.0,
            0.0,
            0.0,
            rng.gen(),
            0.0,
            1.0,
            0.0,
            rng.gen(),
            0.0,
            0.0,
            1.0,
            rng.gen(),
        ],
    };

    CalibrationParameters {
        intrinsics: Some(intrinsics),
        extrinsics: Some(extrinsics),
    }
}

pub async fn hello(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let name = generate_name();

    let which_camera = CameraIdentifier {
        group_name,
        camera_name: name.clone(),
    };

    let calibration = fake_calibration();

    let info = CameraInfo {
        which_camera: Some(which_camera),
        calibration: Some(calibration),
    };

    println!("Camera Info: {:?}", &info);

    let request = Request::new(info);
    println!("Sending request");
    let response = client.hello(request).await?;
    let message = response.into_inner();
    println!("Reply received: {:?}", message);
    Ok(name)
}

/// Generate a random 7-character alphanumeric camera name.
pub(crate) fn generate_name() -> String {
    // From https://docs.rs/rand/0.8.2/rand/distributions/struct.Alphanumeric.html
    let rng = thread_rng();
    rng.sample_iter(Alphanumeric)
        .map(char::from)
        .take(7)
        .collect()
}

// --- Admin request builders (unit-testable) ---

/// Build a CameraIdentifier. Empty camera_name means whole group.
pub fn build_camera_identifier(group_name: String, camera_name: String) -> CameraIdentifier {
    CameraIdentifier {
        group_name,
        camera_name,
    }
}

/// Build ServerSnapshotRequest for TakeSnapshots.
pub fn build_server_snapshot_request(
    group_name: String,
    camera_name: Option<String>,
    with_pose: bool,
    with_image: bool,
    want_pose3d: bool,
) -> ServerSnapshotRequest {
    let which_camera = CameraIdentifier {
        group_name,
        camera_name: camera_name.unwrap_or_default(),
    };
    let params = SnapshotParameters {
        common: Some(SnapshotPayloadParameters {
            with_pose,
            with_image,
        }),
    };
    ServerSnapshotRequest {
        which_camera: Some(which_camera),
        params: Some(params),
        want_pose3d,
    }
}

/// Build StreamControlRequest for starting streaming.
pub fn build_stream_control_start_request(
    group_name: String,
    with_pose: bool,
    with_image: bool,
    fps: Option<f32>,
) -> StreamControlRequest {
    let params = StreamParameters {
        common: Some(SnapshotPayloadParameters {
            with_pose,
            with_image,
        }),
        fps: fps.unwrap_or(0.0),
    };
    StreamControlRequest {
        group_name: group_name.clone(),
        command: Some(stream_control_request::Command::StartStreaming(
            StartStreamingRequest {
                group_name,
                params: Some(params),
            },
        )),
    }
}

/// Build StreamControlRequest for stopping streaming.
pub fn build_stream_control_stop_request(group_name: String) -> StreamControlRequest {
    StreamControlRequest {
        group_name: group_name.clone(),
        command: Some(stream_control_request::Command::StopStreaming(
            StopStreamingRequest { group_name },
        )),
    }
}

/// Build CalibrationRequest.
pub fn build_calibration_request(
    group_name: String,
    camera_name: Option<String>,
    do_extrinsic: bool,
    do_intrinsic: bool,
) -> CalibrationRequest {
    CalibrationRequest {
        which_camera: Some(CameraIdentifier {
            group_name,
            camera_name: camera_name.unwrap_or_default(),
        }),
        command: Some(CalibrateCommand {
            do_extrinsic,
            do_intrinsic,
        }),
    }
}

/// Build PingRequest. None for which_camera means ping no cameras (server returns empty).
pub fn build_ping_request(
    which_camera: Option<CameraIdentifier>,
    timeout_secs: Option<u64>,
) -> PingRequest {
    let timeout = timeout_secs.map(|s| prost_types::Duration {
        seconds: s as i64,
        nanos: 0,
    });
    PingRequest {
        which_camera,
        timeout,
    }
}

// --- Admin RPC wrappers ---

pub async fn list_groups(
    client: &mut HubServiceClient<Channel>,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(ListGroupsRequest {});
    let resp = client.list_groups(req).await?.into_inner();
    Ok(resp.group_names)
}

pub async fn list_cameras(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(ListCamerasRequest { group_name });
    let resp = client.list_cameras(req).await?.into_inner();
    Ok(resp.camera_names)
}

pub async fn get_camera_info(
    client: &mut HubServiceClient<Channel>,
    which_camera: CameraIdentifier,
) -> Result<CameraInfo, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(which_camera);
    let resp = client.get_camera_info(req).await?.into_inner();
    Ok(resp)
}

pub async fn stream_control_start(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
    with_pose: bool,
    with_image: bool,
    fps: Option<f32>,
) -> Result<StreamStatus, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(build_stream_control_start_request(
        group_name, with_pose, with_image, fps,
    ));
    let resp = client.stream_control(req).await?.into_inner();
    Ok(resp)
}

pub async fn stream_control_stop(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<StreamStatus, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(build_stream_control_stop_request(group_name));
    let resp = client.stream_control(req).await?.into_inner();
    Ok(resp)
}

pub async fn take_snapshots(
    client: &mut HubServiceClient<Channel>,
    request: ServerSnapshotRequest,
) -> Result<ServerSnapshotResponse, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(request);
    let resp = client.take_snapshots(req).await?.into_inner();
    Ok(resp)
}

pub async fn get_current(
    client: &mut HubServiceClient<Channel>,
    which_camera: CameraIdentifier,
) -> Result<ServerSnapshotResponse, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(which_camera);
    let resp = client.get_current(req).await?.into_inner();
    Ok(resp)
}

pub async fn calibrate(
    client: &mut HubServiceClient<Channel>,
    request: CalibrationRequest,
) -> Result<CalibrationResponse, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(request);
    let resp = client.calibrate(req).await?.into_inner();
    Ok(resp)
}

pub async fn ping(
    client: &mut HubServiceClient<Channel>,
    request: PingRequest,
) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
    let req = Request::new(request);
    let resp = client.ping(req).await?.into_inner();
    Ok(resp)
}

pub async fn stream_inner(
    group_name: String,
    camera_name: String,
    tx: Sender<Snapshot>,
    rng: &mut ThreadRng,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let nposes: u32 = 1000;

    let which_camera = CameraIdentifier {
        group_name,
        camera_name,
    };

    for i in 0..nposes {
        println!("Sending pose {}", i);
        // TODO: timestamp and image
        let pose_message = Snapshot {
            timestamp: None,
            which_camera: Some(which_camera.clone()),
            poses: vec![rng.gen()],
            image: None,
        };

        tx.try_send(pose_message)?;

        sleep(Duration::from_millis(50)).await;
    }

    Ok(())
}

pub async fn stream_poses(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
    camera_name: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let buf_size = 10;
    let (tx, rx) = channel::<Snapshot>(buf_size);
    let request = Request::new(rx);
    println!("Sending request");

    todo!();
    // let response_future = client.stream_poses(request);
    // let stream_future = stream_inner(group_name, camera_name, tx, &mut rng);
    // let (stream_result, response_result) = join!(stream_future, response_future);
    // stream_result?;
    // println!("Got response: {:#?}", response_result?);

    // Ok(())
}

async fn handle_camera_snapshot_request(
    client: &mut HubServiceClient<Channel>,
    request: CameraControlCommand,
    group_name: String,
    camera_name: String,
    image_data: Option<Image>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    log::info!("Handling snapshot request '{:?}'", &request.token);

    let mut rng = thread_rng();

    let which_camera = CameraIdentifier {
        group_name,
        camera_name,
    };

    // Construct response
    let image_message = Snapshot {
        // Need to clone strings each time we loop
        timestamp: Some(SystemTime::now().into()),
        which_camera: Some(which_camera.clone()),
        poses: vec![rng.gen()],
        image: image_data.or_else(|| Some(rng.gen())),
    };

    // TODO: I think this doesn't fit with new API
    todo!()
}

/// Offer fake snapshots, with the option
/// to send a predefined image instead of
/// random RGB values.
pub async fn offer_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
    image_data: Option<Image>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // TODO: More logging
    let camera_name = generate_name();

    let which_camera = CameraIdentifier {
        group_name,
        camera_name,
    };

    let calibration = fake_calibration();

    // Construct offer
    let offer = CameraInfo {
        which_camera: Some(which_camera),
        calibration: Some(calibration),
    };

    // TODO: fix this
    todo!()
    // Send offer and get stream handle from server
    // let mut stream = client
    //     .wait_for_snapshot_request(Request::new(offer))
    //     .await?
    //     .into_inner();

    // // Iterate over snapshot requests
    // while let Some(snapshot_request) = stream.message().await? {
    //     // TODO: Use server timestamp somehow?
    //     handle_camera_snapshot_request(
    //         client,
    //         snapshot_request,
    //         group_name.clone(),
    //         camera_name.clone(),
    //         image_data.clone(),
    //     )
    //     .await?;
    // }

    // Ok(())
}

/// Request a snapshot from a group (convenience wrapper around take_snapshots).
pub async fn get_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
    with_pose: bool,
    with_image: bool,
    want_pose3d: bool,
) -> Result<ServerSnapshotResponse, Box<dyn std::error::Error + Send + Sync>> {
    let request = build_server_snapshot_request(
        group_name,
        None,
        with_pose,
        with_image,
        want_pose3d,
    );
    take_snapshots(client, request).await
}

#[cfg(test)]
mod tests {
    use super::{
        build_calibration_request, build_camera_identifier, build_ping_request,
        build_server_snapshot_request, build_stream_control_start_request,
        build_stream_control_stop_request, generate_name, stream_control_request,
        CameraIdentifier,
    };

    // Test names are prefixed with "test_" to avoid shadowing the builder functions under test.

    #[test]
    fn generate_name_returns_7_alphanumeric_chars() {
        let name = generate_name();
        assert_eq!(name.len(), 7, "generate_name should return 7 characters");
        assert!(
            name.chars().all(|c| c.is_ascii_alphanumeric()),
            "generate_name should be alphanumeric, got {:?}",
            name
        );
    }

    #[test]
    fn generate_name_returns_different_names() {
        let a = generate_name();
        let b = generate_name();
        // Very unlikely to collide in 7 alphanumeric chars
        assert_ne!(a, b, "generate_name should produce different names");
    }

    #[test]
    fn build_camera_identifier_group_only() {
        let id = build_camera_identifier("g1".to_string(), String::new());
        assert_eq!(id.group_name, "g1");
        assert_eq!(id.camera_name, "");
    }

    #[test]
    fn build_camera_identifier_group_and_camera() {
        let id = build_camera_identifier("g1".to_string(), "cam1".to_string());
        assert_eq!(id.group_name, "g1");
        assert_eq!(id.camera_name, "cam1");
    }

    #[test]
    fn build_server_snapshot_request_group_only() {
        let req = build_server_snapshot_request(
            "my_group".to_string(),
            None,
            true,
            false,
            true,
        );
        assert!(req.which_camera.is_some());
        let id = req.which_camera.as_ref().unwrap();
        assert_eq!(id.group_name, "my_group");
        assert_eq!(id.camera_name, "");
        assert_eq!(req.want_pose3d, true);
        assert!(req.params.is_some());
        let common = req.params.as_ref().unwrap().common.as_ref().unwrap();
        assert!(common.with_pose);
        assert!(!common.with_image);
    }

    #[test]
    fn build_server_snapshot_request_with_camera() {
        let req = build_server_snapshot_request(
            "g".to_string(),
            Some("c1".to_string()),
            false,
            true,
            false,
        );
        let id = req.which_camera.as_ref().unwrap();
        assert_eq!(id.camera_name, "c1");
        let common = req.params.as_ref().unwrap().common.as_ref().unwrap();
        assert!(!common.with_pose);
        assert!(common.with_image);
    }

    #[test]
    fn test_build_stream_control_start_request() {
        let req = build_stream_control_start_request("g1".to_string(), true, true, Some(30.0));
        assert_eq!(req.group_name, "g1");
        match &req.command {
            Some(stream_control_request::Command::StartStreaming(start)) => {
                assert_eq!(start.group_name, "g1");
                assert!(start.params.is_some());
                let p = start.params.as_ref().unwrap();
                assert!(p.common.as_ref().unwrap().with_pose);
                assert!(p.common.as_ref().unwrap().with_image);
                assert_eq!(p.fps, 30.0);
            }
            _ => panic!("expected StartStreaming command"),
        }
    }

    #[test]
    fn build_stream_control_start_request_default_fps() {
        let req = build_stream_control_start_request("g2".to_string(), false, false, None);
        match &req.command {
            Some(stream_control_request::Command::StartStreaming(start)) => {
                assert_eq!(start.params.as_ref().unwrap().fps, 0.0);
            }
            _ => panic!("expected StartStreaming command"),
        }
    }

    #[test]
    fn test_build_stream_control_stop_request() {
        let req = build_stream_control_stop_request("stop_group".to_string());
        assert_eq!(req.group_name, "stop_group");
        match &req.command {
            Some(stream_control_request::Command::StopStreaming(stop)) => {
                assert_eq!(stop.group_name, "stop_group");
            }
            _ => panic!("expected StopStreaming command"),
        }
    }

    #[test]
    fn test_build_calibration_request_with_camera() {
        let req = build_calibration_request(
            "cal_group".to_string(),
            Some("cam1".to_string()),
            true,
            false,
        );
        assert!(req.which_camera.is_some());
        let id = req.which_camera.as_ref().unwrap();
        assert_eq!(id.group_name, "cal_group");
        assert_eq!(id.camera_name, "cam1");
        assert!(req.command.is_some());
        let cmd = req.command.as_ref().unwrap();
        assert!(cmd.do_extrinsic);
        assert!(!cmd.do_intrinsic);
    }

    #[test]
    fn test_build_calibration_request_group_only() {
        let req = build_calibration_request("g".to_string(), None, false, true);
        assert_eq!(req.which_camera.as_ref().unwrap().camera_name, "");
        assert!(req.command.as_ref().unwrap().do_intrinsic);
    }

    #[test]
    fn build_ping_request_no_camera() {
        let req = build_ping_request(None, Some(3));
        assert!(req.which_camera.is_none());
        assert!(req.timeout.is_some());
    }

    #[test]
    fn build_ping_request_with_camera_no_timeout() {
        let id = CameraIdentifier {
            group_name: "g".to_string(),
            camera_name: "c".to_string(),
        };
        let req = build_ping_request(Some(id), None);
        assert!(req.which_camera.is_some());
        assert_eq!(req.which_camera.as_ref().unwrap().group_name, "g");
        assert!(req.timeout.is_none());
    }
}
