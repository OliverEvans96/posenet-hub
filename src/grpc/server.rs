use async_std::channel;
use futures::StreamExt;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::iter;
use std::time::Instant;
use std::{error::Error, net::SocketAddr};
use tonic::{transport::Server, Request, Response, Status, Streaming};

use super::proto::hub_service_server::{HubService, HubServiceServer};
use super::proto::{CameraInfo, Empty, HelloResponse, Pose2D, Pose2DMessage};


pub struct HubServer {
    cameras_tx: channel::Sender<CameraInfo>,
    poses2d_tx: channel::Sender<LabeledPoses2D>,
}

#[derive(Debug)]
pub struct NamedCameraInfo {
    pub name: String,
    pub info: CameraInfo,
}

#[derive(Debug, Clone)]
pub struct LabeledPoses2D {
    pub group_name: String,
    pub camera_name: String,
    pub poses: Vec<Pose2D>,
    pub time: Instant,
}

#[tonic::async_trait]
impl HubService for HubServer {
    async fn hello(&self, request: Request<CameraInfo>) -> Result<Response<HelloResponse>, Status> {
        let info = request.into_inner();
        // let name = generate_name();
        // let camera = NamedCameraInfo {
        //     name: name.clone(),
        //     info,
        // };
        self.cameras_tx
            .send(info)
            .await
            .expect("Pose channel was closed.");

        Ok(Response::new(HelloResponse { }))
    }

    async fn stream_poses(
        &self,
        request: Request<Streaming<Pose2DMessage>>,
    ) -> Result<Response<Empty>, Status> {
        let mut stream = request.into_inner();

        while let Some(message) = stream.next().await {
            let message = message?;
            let poses = message.poses.clone();
            let labeled = LabeledPoses2D {
                group_name: message.group_name.clone(),
                camera_name: message.camera_name.clone(),
                time: Instant::now(),
                poses
            };
            self.poses2d_tx
                .send(labeled)
                .await
                .expect("Pose channel was closed.");
        }

        Ok(Response::new(Empty::default()))
    }
}

pub struct GrpcConfig {
    addr: SocketAddr,
}

impl GrpcConfig {
    pub fn new(ip: &str, port: u16) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            addr: format!("{}:{}", ip, port).parse()?,
        })
    }
}

impl Default for GrpcConfig {
    fn default() -> Self {
        GrpcConfig::new("0.0.0.0", 50051).expect("Default GRPC configuration invalid!")
    }
}

pub struct GrpcServer {
    config: GrpcConfig,
    cameras_tx: channel::Sender<CameraInfo>,
    poses2d_tx: channel::Sender<LabeledPoses2D>,
}

impl GrpcServer {
    pub fn new(
        config: GrpcConfig,
        cameras_tx: channel::Sender<CameraInfo>,
        poses2d_tx: channel::Sender<LabeledPoses2D>,
    ) -> Self {
        Self {
            config,
            cameras_tx,
            poses2d_tx,
        }
    }

    pub async fn run(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("PoseNet Hub gRPC service listening on {}", self.config.addr);

        let hub_server = HubServer {
            cameras_tx: self.cameras_tx,
            poses2d_tx: self.poses2d_tx,
        };

        Server::builder()
            .add_service(HubServiceServer::new(hub_server))
            .serve(self.config.addr)
            .await?;

        Ok(())
    }
}
