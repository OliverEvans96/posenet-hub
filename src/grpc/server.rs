use async_std::channel;
use futures::future::join_all;
use futures::StreamExt;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};
use std::{error::Error, net::SocketAddr};
use tokio::sync::RwLock;
use tonic::{transport::Server, Request, Response, Status, Streaming};
use uuid::Uuid;

use super::proto::hub_service_server::{HubService, HubServiceServer};
use super::proto::{
    CameraInfo, CameraSnapshotRequest, CameraSnapshotResponse, Empty, HelloResponse, Pose2D,
    Pose2DImageMessage, Pose2DMessage, Pose3D, ServerSnapshotRequest, ServerSnapshotResponse,
    SnapshotClientOffer, TriangulationRequest,
};

pub struct HubServer {
    cameras_tx: channel::Sender<CameraInfo>,
    poses2d_tx: channel::Sender<LabeledPoses2D>,
    // snapshot request stream channel senders,
    // indexed by group name, then camera name
    // TODO: Is this the right place to store this?
    // or should we send somewhere else to store?
    snapshot_offers: RwLock<
        HashMap<String, HashMap<String, channel::Sender<Result<CameraSnapshotRequest, Status>>>>,
    >,
    // store snapshot from cameras as they come in,
    // before sending to client once they all arrive (or timeout)
    snapshot_buffers: RwLock<HashMap<String, Vec<Pose2DImageMessage>>>,
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
    // type WaitForSnapshotRequestStream = ReceiverStream<Result<CameraSnapshotRequest, Status>>;
    type WaitForSnapshotRequestStream = channel::Receiver<Result<CameraSnapshotRequest, Status>>;
    type TriangulateStream = channel::Receiver<Result<Pose3D, Status>>;

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

        Ok(Response::new(HelloResponse {}))
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
                poses,
            };
            self.poses2d_tx
                .send(labeled)
                .await
                .expect("Pose channel was closed.");
        }

        Ok(Response::new(Empty::default()))
    }

    async fn wait_for_snapshot_request(
        &self,
        request: tonic::Request<super::proto::SnapshotClientOffer>,
    ) -> Result<tonic::Response<Self::WaitForSnapshotRequestStream>, tonic::Status> {
        match request.into_inner() {
            SnapshotClientOffer {
                group_name,
                camera_name,
            } => {
                // Create channel to send snapshot requests later
                let (tx, rx) = channel::unbounded();

                // New scope here to release lock on snapshot_offers ASAP
                {
                    // Activate lock to gain thread-safe, mutable access to shared data
                    // (this rpc could be called multiple times simultaneously)
                    let mut offers_hm = self.snapshot_offers.write().await;

                    // Get HashMap existing for group if it exists, otherwise create a new one
                    let group_offers = offers_hm.entry(group_name).or_insert_with(HashMap::new);

                    // TODO: Handle repeat offers? (success = false)
                    group_offers.insert(camera_name, tx);

                    // TODO: Remove if camera stops listening to stream?
                }

                Ok(tonic::Response::new(rx))
            }
        }
    }

    async fn send_snapshot(
        &self,
        request: tonic::Request<super::proto::CameraSnapshotResponse>,
    ) -> Result<tonic::Response<super::proto::Empty>, tonic::Status> {
        match request.into_inner() {
            CameraSnapshotResponse {
                snapshot_id,
                message: Some(message),
            } => {
                // Activate lock to gain thread-safe, mutable access to shared data
                // (this rpc could be called multiple times simultaneously)
                // TODO: Don't need to lock whole snapshots_hm, just the inner HM for this snapshot group
                let mut snapshots_hm = self.snapshot_buffers.write().await;

                // Get HashMap existing for group if it exists, otherwise create a new one
                if let Some(snapshots) = snapshots_hm.get_mut(&snapshot_id) {
                    snapshots.push(message);
                } else {
                    // Oops - snapshot_id group didn't exist - maybe we're too late or got a bad snapshot_id?
                    return Err(Status::not_found(format!(
                        "No existing buffer for snapshot_id = {}",
                        &snapshot_id
                    )));
                }
            }
            CameraSnapshotResponse {
                snapshot_id,
                message: None,
            } => {
                // If there's no message, don't do anything
                println!(
                    "Received empty CameraSnapshotResponse for snapshot {:?}",
                    snapshot_id
                );
            }
        }
        Ok(tonic::Response::new(Empty {}))
    }

    async fn get_snapshots(
        &self,
        request: tonic::Request<super::proto::ServerSnapshotRequest>,
    ) -> Result<tonic::Response<super::proto::ServerSnapshotResponse>, tonic::Status> {
        match request.into_inner() {
            ServerSnapshotRequest { group_name } => {
                let snapshot_id = Uuid::new_v4().to_string();

                let (camera_names, send_results) = {
                    let all_offers = self.snapshot_offers.read().await;
                    // Look up all outstanding offers for the requested group
                    if let Some(group_offers) = all_offers.get(&group_name) {
                        let camera_request = CameraSnapshotRequest {
                            // TODO: Can snapshot_id be passed by reference?
                            snapshot_id: snapshot_id.clone(),
                            timestamp: Some(SystemTime::now().into()),
                        };

                        let (camera_names, offer_txs): (Vec<_>, Vec<_>) =
                            group_offers.iter().unzip();

                        // Allocate buffer for snapshot_id before requesting
                        let num_offers = camera_names.len();
                        {
                            let mut all_buffers = self.snapshot_buffers.write().await;
                            all_buffers.insert(snapshot_id.clone(), Vec::with_capacity(num_offers));
                        }

                        // Send requests to relevant cameras & get futures
                        // TODO: Is it necessary to clone camera_request? Can it be passed by reference?
                        let send_futures: Vec<_> = offer_txs
                            .iter()
                            .map(|tx| tx.send(Ok(camera_request.clone())))
                            .collect();

                        // Copy camera names so that we can drop the read lock on self.snapshot_offers
                        let cloned_camera_names = camera_names.into_iter().cloned().collect();

                        // Await futures to actually launch send tasks
                        let send_results = join_all(send_futures).await;

                        (cloned_camera_names, send_results)
                    } else {
                        (vec![], vec![])
                    }
                };

                // Log error if any requests failed to send
                for (send_result, camera_name) in send_results.iter().zip(camera_names) {
                    if let Err(send_err) = send_result {
                        log::warn!(
                            "Sending snapshot request to camera '{}' failed: {:?}",
                            camera_name,
                            send_err
                        );
                    }
                }

                if send_results.len() > 0 {
                    // TODO: How to know when buffer is full without just waiting for timeout?
                    let timeout = Duration::from_secs(3);
                    tokio::time::sleep(timeout).await;
                    let maybe_camera_responses = {
                        let mut buffers_hm = self.snapshot_buffers.write().await;
                        buffers_hm.remove(&snapshot_id)
                    };

                    if let Some(camera_responses) = maybe_camera_responses {
                        let response = ServerSnapshotResponse {
                            snapshot_id,
                            // TODO: need to pass ownership here
                            messages: camera_responses,
                        };
                        Ok(tonic::Response::new(response))
                    } else {
                        // TODO: Is this the right status?
                        Err(Status::not_found(
                            "Received no snapshot responses from cameras before timeout",
                        ))
                    }
                } else {
                    Err(Status::not_found(format!(
                        "No outstanding snapshot offers for the requested group {}",
                        group_name
                    )))
                }
            }
        }
    }

    async fn triangulate(
        &self,
        request: tonic::Request<tonic::Streaming<super::proto::TriangulationRequest>>,
    ) -> Result<tonic::Response<Self::TriangulateStream>, tonic::Status> {
        use crate::triangulator;
        // Collect all poses until client stops streaming
        let mut stream = request.into_inner();
        let mut cameras = Vec::<CameraInfo>::new();
        let mut poses_by_subject = Vec::new();

        let (tx, rx) = channel::unbounded();

        let mut i: u8 = 0;
        while let Some(viewpoint) = stream.message().await? {
            let TriangulationRequest { camera, poses } = viewpoint;
            // Group poses by subject (they arrive grouped by camera)
            if i == 0 {
                for pose in poses {
                    poses_by_subject.push(vec![pose])
                }
            } else {
                if poses.len() == poses_by_subject.len() {
                    for (j, pose) in poses.into_iter().enumerate() {
                        poses_by_subject[j].push(pose);
                    }
                } else {
                    Err(tonic::Status::invalid_argument(
                        "All snapshots must currently have the same number of poses.",
                    ))?;
                }
            }
            // Save camera, too
            if let Some(camera) = camera {
                cameras.push(camera);
            } else {
                Err(tonic::Status::invalid_argument(
                    "All camera data must be present.",
                ))?;
            }
            // Increment counter
            i += 1;
        }

        // Convert CameraInfo objects to Camera Matrices
        let camera_matrices = cameras
            .iter()
            .map(triangulator::calculate_camera_matrix)
            .collect::<Option<Vec<_>>>()
            .ok_or(tonic::Status::invalid_argument(
                "Not all required camera info was provided.",
            ))?;

        // Triangulate and stream 3d poses back to client
        for poses in poses_by_subject {
            let pose3d =
                triangulator::triangulate_from_poses_and_camera_matrices(poses, &camera_matrices);
            // NOTE: Awaiting sequentially to make sure
            // we return the poses in the correct order
            tx.send(Ok(pose3d))
                .await
                .or(Err(tonic::Status::unknown("Failed to return streaming poses")))?;
        }

        // Return receiver to client
        // NOTE: channel happens to already have been populated
        // in this scenario, but in general, more items
        // could be streamed later by keeping `tx` handy
        Ok(tonic::Response::new(rx))
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
        log::info!("PoseNet Hub gRPC service listening on {}", self.config.addr);

        let hub_server = HubServer {
            cameras_tx: self.cameras_tx,
            poses2d_tx: self.poses2d_tx,
            snapshot_offers: RwLock::new(HashMap::new()),
            snapshot_buffers: RwLock::new(HashMap::new()),
        };

        Server::builder()
            .add_service(HubServiceServer::new(hub_server))
            .serve(self.config.addr)
            .await?;

        Ok(())
    }
}
