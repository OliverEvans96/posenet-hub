use rand::distributions::Alphanumeric;
use rand::prelude::ThreadRng;
use rand::{thread_rng, Rng};
use std::time::SystemTime;
use tokio::sync::mpsc::{channel, Sender};
use tokio::time::{sleep, Duration};
use tonic::{transport::Channel, Request};

use super::proto::hub_service_client::HubServiceClient;
use super::proto::*;

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

fn generate_name() -> String {
    // From https://docs.rs/rand/0.8.2/rand/distributions/struct.Alphanumeric.html
    let rng = thread_rng();
    rng.sample_iter(Alphanumeric)
        .map(char::from)
        .take(7)
        .collect()
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

pub async fn get_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<ServerSnapshotResponse, Box<dyn std::error::Error + Send + Sync>> {
    // TODO: camera name?
    let camera_name = String::new();
    let which_camera = CameraIdentifier {
        group_name,
        camera_name,
    };

    let request = todo!();
}

pub async fn get_snapshot_cameras(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<ServerSnapshotResponse, Box<dyn std::error::Error + Send + Sync>> {
    // TODO: camera name?
    let camera_name = String::new();
    let which_camera = CameraIdentifier {
        group_name,
        camera_name,
    };

    // TODO: Other params?
    let common = SnapshotPayloadParameters {
        with_pose: true,
        with_image: false,
    };
    let params = SnapshotParameters {
        common: Some(common),
    };

    let request = ServerSnapshotRequest {
        which_camera: Some(which_camera),
        params: Some(params),
        want_pose3d: true,
    };

    todo!()
    // let response = client.get_snapshot_cameras(request).await?.into_inner();
    // Ok(response)
}
