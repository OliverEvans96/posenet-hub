use async_std::channel;
use futures::StreamExt;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::error::Error;
use std::iter;
use tonic::{transport::Server, Request, Response, Status, Streaming};

use super::proto::hub_service_server::{HubService, HubServiceServer};
use super::proto::{CameraInfo, Empty, HelloResponse, Pose2DMessage};

fn generate_name() -> String {
    // From https://docs.rs/rand/0.8.2/rand/distributions/struct.Alphanumeric.html
    let mut rng = thread_rng();
    iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .map(char::from)
        .take(7)
        .collect()
}

pub struct HubServer {
    cameras_tx: channel::Sender<NamedCameraInfo>,
    poses_tx: channel::Sender<Pose2DMessage>,
}

#[derive(Debug)]
pub struct NamedCameraInfo {
    name: String,
    info: CameraInfo,
}

#[tonic::async_trait]
impl HubService for HubServer {
    async fn hello(&self, request: Request<CameraInfo>) -> Result<Response<HelloResponse>, Status> {
        let info = request.into_inner();
        let name = generate_name();
        let camera = NamedCameraInfo {
            name: name.clone(),
            info,
        };

        self.cameras_tx
            .send(camera)
            .await
            .expect("Pose channel was closed.");

        Ok(Response::new(HelloResponse { name }))
    }

    async fn stream_poses(
        &self,
        request: Request<Streaming<Pose2DMessage>>,
    ) -> Result<Response<Empty>, Status> {
        let mut stream = request.into_inner();

        while let Some(message) = stream.next().await {
            let message = message?;
            self.poses_tx
                .send(message)
                .await
                .expect("Pose channel was closed.");
        }

        Ok(Response::new(Empty::default()))
    }
}

pub async fn main(
    cameras_tx: channel::Sender<NamedCameraInfo>,
    poses_tx: channel::Sender<Pose2DMessage>,
) -> Result<(), Box<dyn Error>> {
    let addr = "[::1]:50051".parse()?;
    let hub_server = HubServer {
        cameras_tx,
        poses_tx,
    };

    println!("PoseNet Hub gRPC service listening on {}", addr);

    Server::builder()
        .add_service(HubServiceServer::new(hub_server))
        .serve(addr)
        .await?;

    Ok(())
}
