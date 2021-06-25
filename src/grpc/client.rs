use async_std;
use rand::{thread_rng, Rng};
use std::error::Error;
use tokio::{
    join,
    time::{sleep, Duration},
};
use tonic::{transport::Channel, Request};

use super::proto::hub_service_client::HubServiceClient;
use super::proto::{CameraExtrinsics, CameraIntrinsics, CameraInfo};
use super::proto::{Point2D, Point3D, Pose2D, Pose2DMessage};

pub async fn hello(client: &mut HubServiceClient<Channel>) -> Result<String, Box<dyn Error>> {
    let mut rng = thread_rng();
    let camera_info = CameraInfo {
        intrinsics: Some(CameraIntrinsics {
            camera_matrix: vec![100.0,0.0,0.0,0.0,100.0,0.0,0.0,0.0,100.0],
            distortion: vec![0.0,0.0,0.0,0.0,0.0],
            rms_error: 0.0
        }),
        extrinsics: Some(CameraExtrinsics {
            view_matrix: vec![1.0,0.0,0.0,rng.gen(),0.0,1.0,0.0,rng.gen(),0.0,0.0,1.0,rng.gen()]
        }),
        needs_intrinsic_calibration: false,
        needs_extrinsic_calibration: false
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
            score: 1.0
        }),
        left_eye: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        right_eye: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        left_ear: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        right_ear: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        left_shoulder: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        right_shoulder: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        left_elbow: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        right_elbow: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        left_wrist: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        right_wrist: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        left_hip: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        right_hip: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        left_knee: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        right_knee: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        left_ankle: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        right_ankle: Some(Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: 1.0
        }),
        score: 1.0
    }
}

pub async fn stream_inner(
    name: String,
    tx: async_std::channel::Sender<Pose2DMessage>,
) -> Result<(), Box<dyn Error>> {
    let nposes: u32 = 1000;
    for i in 0..nposes {
        println!("Sending pose {}", i);
        let pose_message = Pose2DMessage {
            camera_name: name.clone(),
            poses: vec![random_pose()],
        };

        tx.try_send(pose_message)?;

        sleep(Duration::from_millis(50)).await;
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
