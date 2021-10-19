use async_std;
use rand::distributions::Alphanumeric;
use rand::prelude::ThreadRng;
use rand::{thread_rng, Rng};
use std::error::Error;
use std::time::SystemTime;
use tokio::{
    join,
    time::{sleep, Duration},
};
use tonic::{transport::Channel, Request};

use super::proto::hub_service_client::HubServiceClient;
use super::proto::{CameraExtrinsics, CameraInfo, CameraIntrinsics};
use super::proto::{
    CameraSnapshotRequest, CameraSnapshotResponse, ServerSnapshotRequest, ServerSnapshotResponse,
    SnapshotCamerasResponse,
};
use super::proto::{ImageData, Pose2DImageMessage, Pose2DMessage};

pub async fn hello(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<String, Box<dyn Error>> {
    let mut rng = thread_rng();
    let name = generate_name();
    let camera_info = CameraInfo {
        intrinsics: Some(CameraIntrinsics {
            camera_matrix: vec![100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0],
            distortion: vec![0.0, 0.0, 0.0, 0.0, 0.0],
            rms_error: 0.0,
        }),
        extrinsics: Some(CameraExtrinsics {
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
        }),
        group_name: group_name,
        camera_name: name.clone(),
    };
    println!("Camera Info: {:?}", camera_info);

    let request = Request::new(camera_info);
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
    tx: async_std::channel::Sender<Pose2DMessage>,
    rng: &mut ThreadRng,
) -> Result<(), Box<dyn Error>> {
    let nposes: u32 = 1000;
    for i in 0..nposes {
        println!("Sending pose {}", i);
        let pose_message = Pose2DMessage {
            group_name: group_name.clone(),
            camera_name: camera_name.clone(),
            poses: vec![rng.gen()],
        };

        tx.try_send(pose_message)?;

        sleep(Duration::from_millis(50)).await;
    }

    tx.close();

    Ok(())
}

pub async fn stream_poses(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
    camera_name: String,
) -> Result<(), Box<dyn Error>> {
    let mut rng = thread_rng();
    let buf_size = 10;
    let (tx, rx) = async_std::channel::bounded::<Pose2DMessage>(buf_size);
    let request = Request::new(rx);
    println!("Sending request");
    let response_future = client.stream_poses(request);
    let stream_future = stream_inner(group_name, camera_name, tx, &mut rng);
    let (stream_result, response_result) = join!(stream_future, response_future);
    stream_result?;
    println!("Got response: {:#?}", response_result?);

    Ok(())
}

async fn handle_camera_snapshot_request(
    client: &mut HubServiceClient<Channel>,
    request: CameraSnapshotRequest,
    group_name: String,
    camera_name: String,
    image_data: Option<ImageData>,
) -> Result<(), Box<dyn Error>> {
    log::info!("Handling snapshot request '{}'", &request.snapshot_id);

    let mut rng = thread_rng();

    // Construct response
    let image_message = Pose2DImageMessage {
        // Need to clone strings each time we loop
        group_name: group_name.clone(),
        camera_name: camera_name.clone(),
        poses: vec![rng.gen()],
        image: image_data.or_else(|| Some(rng.gen())),
        timestamp: Some(SystemTime::now().into()),
    };

    let snapshot_response = CameraSnapshotResponse {
        snapshot_id: request.snapshot_id.clone(),
        message: Some(image_message),
    };

    // Send snapshot to server
    client
        .send_snapshot(Request::new(snapshot_response))
        .await?;

    log::info!(
        "Finished handling snapshot request '{}'",
        request.snapshot_id
    );

    Ok(())
}

/// Offer fake snapshots, with the option
/// to send a predefined image instead of
/// random RGB values.
pub async fn offer_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
    image_data: Option<ImageData>,
) -> Result<(), Box<dyn Error>> {
    // TODO: More logging
    let camera_name = generate_name();

    // Construct offer
    let offer = CameraInfo {
        group_name: group_name.clone(),
        camera_name: camera_name.clone(),
        ..Default::default()
    };

    // Send offer and get stream handle from server
    let mut stream = client
        .wait_for_snapshot_request(Request::new(offer))
        .await?
        .into_inner();

    // Iterate over snapshot requests
    while let Some(snapshot_request) = stream.message().await? {
        // TODO: Use server timestamp somehow?
        handle_camera_snapshot_request(
            client,
            snapshot_request,
            group_name.clone(),
            camera_name.clone(),
            image_data.clone(),
        )
        .await?;
    }

    Ok(())
}

pub async fn get_snapshots(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<ServerSnapshotResponse, Box<dyn Error>> {
    let request = ServerSnapshotRequest { group_name };
    let response = client.get_snapshots(request).await?.into_inner();
    Ok(response)
}

pub async fn get_snapshot_cameras(
    client: &mut HubServiceClient<Channel>,
    group_name: String,
) -> Result<SnapshotCamerasResponse, Box<dyn Error>> {
    let request = ServerSnapshotRequest { group_name };
    let response = client.get_snapshot_cameras(request).await?.into_inner();
    Ok(response)
}
