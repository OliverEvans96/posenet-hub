use tonic::{transport::Server, Request, Response, Status, Streaming};

use proto::hub_service_server::{HubService, HubServiceServer};
use proto::{CameraInfo, Empty, HelloResponse, Pose3D};

use futures::StreamExt;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::iter;

pub mod proto {
    tonic::include_proto!("posenet_vr");
}

fn generate_name() -> String {
    // From https://docs.rs/rand/0.8.2/rand/distributions/struct.Alphanumeric.html
    let mut rng = thread_rng();
    iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .map(char::from)
        .take(7)
        .collect()
}

#[derive(Debug, Default)]
pub struct HubServer {}

#[tonic::async_trait]
impl HubService for HubServer {
    async fn hello(&self, request: Request<CameraInfo>) -> Result<Response<HelloResponse>, Status> {
        let message = request.into_inner();
        println!("Got a request: {:#?}", message);

        // TODO: Store name and camera info
        let name = generate_name();
        let intr = message.intrinsics;
        println!("name = {}", name);
        println!("intr = {:?}", intr);

        let reply = HelloResponse { name: name.into() };

        Ok(Response::new(reply))
    }

    async fn stream_poses(
        &self,
        request: Request<Streaming<Pose3D>>,
    ) -> Result<Response<Empty>, Status> {
        println!("StreamPoses");

        let mut stream = request.into_inner();

        while let Some(pose3d) = stream.next().await {
            let pose3d = pose3d?;

            println!("Got pose: {:?}", pose3d);
        }

        Ok(Response::new(Empty::default()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let hub_server = HubServer::default();

    println!("Greeter service listening on {}", addr);

    Server::builder()
        .add_service(HubServiceServer::new(hub_server))
        .serve(addr)
        .await?;

    Ok(())
}
