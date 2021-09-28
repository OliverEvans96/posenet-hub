use async_std;
use std::time::SystemTime;
use rand::{thread_rng, Rng};
use rand::distributions::{Alphanumeric};
use rand::prelude::ThreadRng;
use std::error::Error;
use tokio::{
    join,
    time::{sleep, Duration},
};
use tonic::{transport::Channel, Request};

use super::proto::hub_service_client::HubServiceClient;
use super::proto::{CameraExtrinsics, CameraIntrinsics, CameraInfo};
use super::proto::{SnapshotClientOffer, CameraSnapshotResponse};
use super::proto::{Point2D, Pose2D, Pose2DMessage, ImageData, Pose2DImageMessage};

pub async fn hello(client: &mut HubServiceClient<Channel>, group_name: &str) -> Result<String, Box<dyn Error>> {
    let mut rng = thread_rng();
    let name = &generate_name();
    let camera_info = CameraInfo {
        intrinsics: Some(CameraIntrinsics {
            camera_matrix: vec![100.0,0.0,0.0,0.0,100.0,0.0,0.0,0.0,100.0],
            distortion: vec![0.0,0.0,0.0,0.0,0.0],
            rms_error: 0.0
        }),
        extrinsics: Some(CameraExtrinsics {
            view_matrix: vec![1.0,0.0,0.0,rng.gen(),0.0,1.0,0.0,rng.gen(),0.0,0.0,1.0,rng.gen()]
        }),
        group_name: group_name.to_string(), 
        camera_name: name.to_string()
    };
    println!("Camera Info: {:?}", camera_info);

    let request = Request::new(camera_info);
    let response_promise = client.hello(request);
    println!("Message sent.");

    let response = response_promise.await?;
    let message = response.into_inner();
    println!("Reply received: {:?}", message);
    Ok(name.to_string())
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
    group_name: &str,
    camera_name: &str,
    tx: async_std::channel::Sender<Pose2DMessage>,
    rng: &mut ThreadRng,
) -> Result<(), Box<dyn Error>> {
    let nposes: u32 = 1000;
    for i in 0..nposes {
        println!("Sending pose {}", i);
        let pose_message = Pose2DMessage {
            group_name: group_name.to_string(),
            camera_name: camera_name.to_string(),
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
    group_name: &str,
    camera_name: &str
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

pub async fn wait_for_snapshot_request(
    client: &mut HubServiceClient<Channel>, 
    group_name: String
) -> Result<(), Box<dyn Error>> {
    let mut rng = thread_rng();
    let camera_name = generate_name();

    // Construct offer
    let offer = SnapshotClientOffer {
        group_name: group_name.clone(),
        camera_name: camera_name.clone()
    };

    // Send offer and get stream handle from server
    let mut stream = client.wait_for_snapshot_request(Request::new(offer)).await?.into_inner();

    // Iterate over snapshot requests
    while let Some(snapshot_request) = stream.message().await? {
        // TODO: Use server timestamp somehow?

        // Construct response
        let image_message = Pose2DImageMessage {
            // Need to clone strings each time we loop
            group_name: group_name.clone(),
            camera_name: camera_name.clone(),
            poses: vec![rng.gen()],
            image: Some(rng.gen()),
            timestamp: Some(SystemTime::now().into()),
        };

        let snapshot_response = CameraSnapshotResponse {
            snapshot_id: snapshot_request.snapshot_id,
            message: Some(image_message),
        };

        // Send snapshot to server
        client.send_snapshot(Request::new(snapshot_response)).await?;
    }

    Ok(())
}



pub mod tests {
    #[test]
    fn test_random_image() {
        use rand::{thread_rng,Rng};
        use super::ImageData;

        let mut rng = thread_rng();
        let image: ImageData = rng.gen();
        println!("{:?}", image);
    }
}