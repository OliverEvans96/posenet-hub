use async_std::channel;
use futures::future::join_all;
use futures::StreamExt;
use nalgebra::{Matrix2xX, Matrix3, Matrix3xX, Matrix4, Point3, Vector2, Vector3};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};
use std::{error::Error, net::SocketAddr};
use tokio::select;
use tokio::sync::RwLock;
use tonic::{transport::Server, Request, Response, Status, Streaming};
use uuid::Uuid;

use crate::openmvg::openmvg::ceres_bundle_adjustment;

use super::proto::hub_service_server::{HubService, HubServiceServer};
use super::proto::{
    BundleAdjustmentResponse, CameraExtrinsics, CameraInfo, CameraIntrinsics,
    CameraSnapshotRequest, CameraSnapshotResponse, Empty, HelloResponse, Pose2D,
    Pose2DImageMessage, Pose2DMessage, Pose3D, SPoint2, SPoint3, ServerSnapshotRequest,
    ServerSnapshotResponse, SnapshotClientOffer, TriangulationRequest, TriangulationResponse,
};

pub struct HubServer {
    cameras_tx: channel::Sender<CameraInfo>,
    poses2d_tx: channel::Sender<LabeledPoses2D>,
    /// snapshot request stream channel senders,
    /// indexed by group name, then camera name
    ///
    /// TODO: Is this the right place to store this?
    /// or should we send somewhere else to store?
    snapshot_offers: RwLock<
        HashMap<String, HashMap<String, channel::Sender<Result<CameraSnapshotRequest, Status>>>>,
    >,
    /// Channels where incoming snapshots can be placed between
    /// get-snapshots request and response.
    snapshot_channels: RwLock<HashMap<String, channel::Sender<Pose2DImageMessage>>>,
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

    async fn hello(&self, request: Request<CameraInfo>) -> Result<Response<HelloResponse>, Status> {
        let info = request.into_inner();
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
                let channels_hm = self.snapshot_channels.read().await;

                // Get HashMap existing for group if it exists, otherwise create a new one
                if let Some(sender) = channels_hm.get(&snapshot_id) {
                    if let Err(err) = sender.send(message).await {
                        return Err(Status::unknown(format!(
                            "Failed to send on snapshot channel: {}",
                            err
                        )));
                    }
                } else {
                    // Oops - snapshot_id group didn't exist - maybe we're too late or got a bad snapshot_id?
                    return Err(Status::not_found(format!(
                        "No existing chanel for snapshot_id = {}",
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
        let ServerSnapshotRequest { group_name } = request.into_inner();
        let snapshot_id = Uuid::new_v4().to_string();

        log::info!("Snapshot request {}", snapshot_id);

        // Introduce new scope here to drop RwLock on snapshot_offers ASAP
        let (camera_names, send_results, rx) = {
            let all_offers = self.snapshot_offers.read().await;

            // Look up all outstanding offers for the requested group
            let group_offers = all_offers
                .get(&group_name)
                .ok_or(tonic::Status::unavailable(format!(
                    "No outstanding snapshot offers for group {}",
                    group_name
                )))?;
            let camera_request = CameraSnapshotRequest {
                // TODO: Can snapshot_id be passed by reference?
                snapshot_id: snapshot_id.clone(),
                timestamp: Some(SystemTime::now().into()),
            };

            let (camera_names, offer_txs): (Vec<_>, Vec<_>) = group_offers
                .iter()
                // Filter out closed offers
                // TODO: Better to actually remove closed offers from offers_hm (which this is not doing)
                // when camera disconnects by implementing a custom Stream wrapper.
                //  See https://github.com/hyperium/tonic/issues/377
                .filter(|(_, offer)| !offer.is_closed())
                .unzip();

            // Create channel for snapshot_id before requesting
            let num_offers = camera_names.len();
            let (tx, rx) = channel::bounded(num_offers);
            {
                let mut all_channels = self.snapshot_channels.write().await;
                all_channels.insert(snapshot_id.clone(), tx);
            }

            // Send requests to relevant cameras & get futures
            // TODO: Is it necessary to clone camera_request? Can it be passed by reference?
            let send_futures: Vec<_> = offer_txs
                .iter()
                .map(|tx| tx.send(Ok(camera_request.clone())))
                .collect();

            // Copy camera names so that we can drop the read lock on self.snapshot_offers
            let cloned_camera_names: Vec<_> = camera_names.into_iter().cloned().collect();

            // Await futures to actually launch send tasks
            let send_results = join_all(send_futures).await;

            (cloned_camera_names, send_results, rx)
        };

        let num_offers = camera_names.len();

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
            let mut camera_responses = vec![];

            async fn collect_poses(
                rx: channel::Receiver<Pose2DImageMessage>,
                vec: &mut Vec<Pose2DImageMessage>,
                num_poses: usize,
            ) {
                // TODO: Wait until closed, or
                for _ in 0..num_poses {
                    match rx.recv().await {
                        Ok(pose) => vec.push(pose),
                        Err(err) => {
                            log::error!("Error while collecting poses from channel: {}", err)
                        }
                    }
                }
            }

            // Collect as many poses as possible before the timeout,
            // returning early if we get as many as expected.
            let timeout = Duration::from_secs(3);
            select! {
                _ = collect_poses(rx, &mut camera_responses, num_offers) => {},
                _ = tokio::time::sleep(timeout) => {},
            }

            // Remove channel
            {
                let mut channels_hm = self.snapshot_channels.write().await;
                channels_hm.remove(&snapshot_id)
            };

            if camera_responses.len() > 0 {
                let response = ServerSnapshotResponse {
                    snapshot_id,
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

    async fn triangulate(
        &self,
        request: tonic::Request<tonic::Streaming<super::proto::TriangulationRequest>>,
    ) -> Result<tonic::Response<super::proto::TriangulationResponse>, tonic::Status> {
        use crate::triangulator;
        // Collect all poses until client stops streaming
        let mut stream = request.into_inner();
        let mut cameras = Vec::<CameraInfo>::new();
        let mut poses_by_subject = Vec::new();

        log::info!("Got triangulate request");

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
                cameras.push(camera.into());
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

        let poses3d = poses_by_subject.into_iter().map(|poses| {
            triangulator::triangulate_from_poses_and_camera_matrices(poses, &camera_matrices)
        });

        let response = TriangulationResponse {
            poses: poses3d.collect(),
        };

        log::info!("Finished triangulate request");

        Ok(Response::new(response))
    }

    async fn bundle_adjustment(
        &self,
        request: tonic::Request<super::proto::BundleAdjustmentRequest>,
    ) -> Result<tonic::Response<super::proto::BundleAdjustmentResponse>, tonic::Status> {
        let message = request.into_inner();
        let nkeypoints = 17;
        let views = message.views;
        let nviews = views.len();
        let nposes = if nviews > 0 { views[0].poses.len() } else { 0 };

        // Combine all poses into single matrix
        let npoints_total = nkeypoints * nposes;
        let mut xs = Vec::with_capacity(nviews);
        let mut x3d = Matrix3xX::zeros(npoints_total);
        let mut ks = Vec::with_capacity(nviews);
        let mut rs = Vec::with_capacity(nviews);
        let mut ts = Vec::with_capacity(nviews);

        // Collect initial guess
        for (k, initial_pose) in message.initial_poses.into_iter().enumerate() {
            let (spoints, _): (Vec<SPoint3>, f64) = initial_pose.into();
            for (h, (point, _score)) in spoints.iter().enumerate() {
                let j = nkeypoints * k + h;
                let col = Vector3::new(point.x, point.y, point.z);
                x3d.set_column(j, &col);
            }
        }

        let mut orig_cameras = Vec::with_capacity(nviews);
        for view in views {
            // Collect keypoint observations from this camera
            let mut x = Matrix2xX::zeros(npoints_total);
            for (k, pose) in view.poses.into_iter().enumerate() {
                let (spoints, _): (Vec<SPoint2>, f64) = pose.into();
                for (h, (point, _score)) in spoints.iter().enumerate() {
                    let j = nkeypoints * k + h;
                    // TODO: Use score
                    let col = Vector2::new(point.x, point.y);
                    x.set_column(j, &col);
                }
            }
            xs.push(x);

            // TODO: Don't panic
            let camera = view.camera.unwrap();
            // TODO: Use distortion coefficients
            let k = Matrix3::from_row_slice(&camera.intrinsics.as_ref().unwrap().camera_matrix);
            let c = Matrix4::from_row_slice(&camera.extrinsics.as_ref().unwrap().view_matrix);
            let cn = c.fixed_rows::<3>(0);
            // TODO: Avoid copying here?
            let r = cn.fixed_columns::<3>(0).clone_owned();
            let t = cn.column(3).clone_owned();

            ks.push(k);
            rs.push(r);
            ts.push(t);

            // Save camera for later
            orig_cameras.push(camera);
        }

        let result = ceres_bundle_adjustment(&xs, &mut ks, &mut ts, &mut rs, &mut x3d);

        if !result {
            return Err(Status::internal("Bundle adjustment failed"));
        }

        // Unpack results back into protobuf types
        let mut cameras = Vec::with_capacity(nviews);
        for (i, orig_camera) in orig_cameras.into_iter().enumerate() {
            let camera_name = orig_camera.camera_name;
            let group_name = orig_camera.group_name;
            let distortion = orig_camera
                .intrinsics
                .map(|int| int.distortion)
                .unwrap_or(vec![]);

            // Collect view matrix from r & t
            let r = rs[i];
            let t = ts[i];
            let mut c = Matrix4::identity();
            let mut cn = c.fixed_rows_mut::<3>(0);
            cn.fixed_columns_mut::<3>(0).copy_from(&r);
            cn.column_mut(3).copy_from(&t);
            // We want row-major order, but nalgebra gives us column-major,
            // so transpose first.
            let view_matrix = c.transpose().as_slice().to_vec();

            // Collect camera matrix
            let camera_matrix = ks[i].transpose().as_slice().to_vec();
            let camera = CameraInfo {
                extrinsics: Some(CameraExtrinsics { view_matrix }),
                intrinsics: Some(CameraIntrinsics {
                    camera_matrix,
                    distortion,
                    rms_error: 0.0, // Not sure what to do with this
                }),
                camera_name,
                group_name,
            };
            cameras.push(camera);
        }

        // Unpack poses
        let mut poses = Vec::with_capacity(nposes);
        for k in 0..nposes {
            let mut spoints = Vec::with_capacity(nkeypoints);
            for h in 0..nkeypoints {
                let j = nkeypoints * k + h;
                let col = x3d.column(j);
                let point = Point3::from_slice(col.as_slice());
                // TODO: What to use for score?
                let score = 1.0;
                let spoint = (point, score);
                spoints.push(spoint);
            }
            // TODO: What to use for score?
            let score = 1.0;
            let pose: Pose3D = (spoints, score).into();
            poses.push(pose)
        }

        // Send response
        let response = BundleAdjustmentResponse { cameras, poses };
        Ok(Response::new(response))
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
            snapshot_channels: RwLock::new(HashMap::new()),
        };

        Server::builder()
            .add_service(HubServiceServer::new(hub_server))
            .serve(self.config.addr)
            .await?;

        Ok(())
    }
}
