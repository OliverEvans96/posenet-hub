use async_std;
use rand::{thread_rng, Rng};
use std::error::Error;
use tokio::{
    join,
    time::{delay_for, Duration},
};
use tonic::{transport::Channel, Request};

use super::proto::hub_service_client::HubServiceClient;
use super::proto::{CameraExtrinsics, CameraInfo};
use super::proto::{EulerAngles, Point2D, Point3D, Pose2D, Pose2DMessage};

pub async fn hello(client: &mut HubServiceClient<Channel>) -> Result<String, Box<dyn Error>> {
    let mut rng = thread_rng();
    let camera_info = CameraInfo {
        intrinsics: None,
        extrinsics: Some(CameraExtrinsics {
            position: Some(Point3D {
                x: rng.gen(),
                y: rng.gen(),
                z: rng.gen(),
            }),
            orientation: Some(EulerAngles {
                yaw: rng.gen(),
                pitch: rng.gen(),
                roll: rng.gen(),
            }),
        }),
    };
    println!("Camera Info: {:?}", camera_info);

    let request = Request::new(camera_info);
    let response_promise = client.hello(request);
    println!("Message sent.");

    let response = response_promise.await?;
    let message = response.into_inner();
    println!("Reply received: {:?}", message);
    Ok(message.name)
}

pub fn random_pose() -> Pose2D {
    let mut rng = thread_rng();
    Pose2D {
        nose: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        left_eye: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        right_eye: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        left_ear: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        right_ear: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        left_shoulder: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        right_shoulder: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        left_elbow: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        right_elbow: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        left_wrist: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        right_wrist: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        left_hip: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        right_hip: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        left_knee: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        right_knee: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        left_ankle: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
        right_ankle: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
        }),
    }
}

pub async fn stream_inner(
    name: String,
    tx: async_std::channel::Sender<Pose2DMessage>,
) -> Result<(), Box<dyn Error>> {
    let nposes: u32 = 15;
    for i in 0..nposes {
        println!("Sending pose {}", i);
        let pose_message = Pose2DMessage {
            camera_name: name.clone(),
            pose: Some(random_pose()),
        };

        tx.try_send(pose_message)?;

        delay_for(Duration::from_millis(500)).await;
    }

    tx.close();

    Ok(())
}

pub async fn stream_poses(
    client: &mut HubServiceClient<Channel>,
    name: String,
) -> Result<(), Box<dyn Error>> {
    let buf_size = 10;
    let (tx, rx) = async_std::channel::bounded::<Pose2DMessage>(buf_size);
    let request = Request::new(rx);
    println!("Sending request");
    let response_future = client.stream_poses(request);
    let stream_future = stream_inner(name, tx);
    let (stream_result, response_result) = join!(stream_future, response_future);
    stream_result?;
    println!("Got response: {:#?}", response_result?);

    Ok(())
}
