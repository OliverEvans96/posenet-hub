use futures::future::join_all;
use futures::{pin_mut, Future, FutureExt, Stream, StreamExt};
use nalgebra::{Matrix2xX, Matrix3, Matrix3xX, Matrix4, Point3, Vector2, Vector3};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::convert::TryInto;
use std::task::Poll;
use std::time::{Duration, Instant};
use std::{net::SocketAddr, pin::Pin};
use thiserror::Error;
use tokio::select;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Request, Response, Status};

use crate::errors::{ConfigError, HubError, InvalidInput, MissingField};
use crate::openmvg::openmvg::ceres_bundle_adjustment;

use super::proto::hub_service_server::{HubService, HubServiceServer};
use super::proto::*;

pub struct HubServer {
    // TODO: Are these still necessary? Redundant with new structs at all?
    cameras_tx: mpsc::Sender<CameraInfo>,
    snapshots_tx: mpsc::Sender<Snapshot>,
    /// snapshot request stream channel senders,
    /// indexed by group name, then camera name
    ///
    /// TODO: Is this the right place to store this?
    /// or should we send somewhere else to store?
    // snapshot_offers: RwLock<HashMap<String, HashMap<String, SnapshotOffer>>>, // TODO: Remove
    /// Channels where incoming snapshots can be placed between
    /// get-snapshots request and response.
    // snapshot_channels: RwLock<HashMap<String, mpsc::Sender<Snapshot>>>, // TODO: Remove

    /// active camera session tokens
    /// indexed by group name, then camera name
    session_tokens: RwLock<HashMap<String, RwLock<HashMap<String, SessionToken>>>>,
    /// Latest streamed snapshots
    /// indexed by group name, then camera name
    stream_cache: RwLock<HashMap<String, RwLock<HashMap<String, Snapshot>>>>,
    /// Active camera control channels, indexed by session token
    /// TODO: Use token data as key, not whole token?
    control_channels: RwLock<HashMap<SessionToken, mpsc::Sender<CameraControlCommand>>>,
    /// Channels to route incoming data from cameras, indexed by command token
    data_channels: RwLock<HashMap<CommandToken, mpsc::Sender<CommandResponseMessage>>>,
}

#[tonic::async_trait]
impl HubService for HubServer {
    // TODO: Create custom stream type that notifies hub server when client disconnects.
    type CameraControlStream = ReceiverStream<Result<CameraControlCommand, Status>>;

    // Old

    /*
    async fn stream_poses(
        &self,
        request: Request<Streaming<Pose2DMessage>>,
    ) -> Result<Response<Empty>, Status> {
        let mut stream = request.into_inner();

        while let Some(message) = stream.next().await {
            let message = message?;
            let poses = message.poses.clone();
            let labeled = Snapshot {
                group_name: message.group_name.clone(),
                camera_name: message.camera_name.clone(),
                time: Instant::now(),
                poses,
            };
            self.snapshots_tx
                .send(labeled)
                .await
                .expect("Pose channel was closed.");
        }

        Ok(Response::new(Empty::default()))
    }

    async fn wait_for_snapshot_request(
        &self,
        request: Request<CameraInfo>,
    ) -> Result<Response<Self::WaitForSnapshotRequestStream>, Status> {
        let camera = request.into_inner();
        // Create channel to send snapshot requests later
        let (tx, rx) = mpsc::unbounded_channel();

        // New scope here to release lock on snapshot_offers ASAP
        {
            // Activate lock to gain thread-safe, mutable access to shared data
            // (this rpc could be called multiple times simultaneously)
            let mut offers_hm = self.snapshot_offers.write();

            let group_name = camera.group_name.clone();
            let camera_name = camera.camera_name.clone();

            // Get HashMap existing for group if it exists, otherwise create a new one
            let group_offers = offers_hm.entry(group_name).or_insert_with(HashMap::new);

            let offer = SnapshotOffer { camera, snd: tx };

            // TODO: Handle repeat offers? (success = false)
            group_offers.insert(camera_name, offer);

            // TODO: Remove if camera stops listening to stream?
        }

        Ok(Response::new(rx))
    }

    async fn send_snapshot(
        &self,
        request: Request<CameraSnapshotResponse>,
    ) -> Result<Response<Empty>, Status> {
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
                log::warn!(
                    "Received empty CameraSnapshotResponse for snapshot {:?}",
                    snapshot_id
                );
            }
        }
        Ok(Response::new(Empty {}))
    }

    async fn get_snapshots(
        &self,
        request: Request<ServerSnapshotRequest>,
    ) -> Result<Response<ServerSnapshotResponse>, Status> {
        let ServerSnapshotRequest { group_name } = request.into_inner();
        let snapshot_id = Uuid::new_v4().to_string();

        log::info!("Snapshot request {}", snapshot_id);

        // Introduce new scope here to drop RwLock on snapshot_offers ASAP
        let (camera_names, send_results, rx) = {
            let all_offers = self.snapshot_offers.read().await;

            // Look up all outstanding offers for the requested group
            let group_offers = all_offers
                .get(&group_name)
                .ok_or(Status::unavailable(format!(
                    "No outstanding snapshot offers for group {}",
                    group_name
                )))?;
            let camera_request = CameraSnapshotRequest {
                // TODO: Can snapshot_id be passed by reference?
                snapshot_id: snapshot_id.clone(),
                timestamp: Some(SystemTime::now().into()),
            };

            let (camera_names, offers): (Vec<_>, Vec<_>) = group_offers
                .iter()
                // Filter out closed offers
                // TODO: Better to actually remove closed offers from offers_hm (which this is not doing)
                // when camera disconnects by implementing a custom Stream wrapper.
                //  See https://github.com/hyperium/tonic/issues/377
                .filter(|(_, offer)| !offer.snd.is_closed())
                .unzip();

            // Create channel for snapshot_id before requesting
            let num_offers = camera_names.len();
            let (tx, rx) = mpsc::channel(num_offers);
            {
                let mut all_channels = self.snapshot_channels.write();
                all_channels.insert(snapshot_id.clone(), tx);
            }

            // Send requests to relevant cameras & get futures
            // TODO: Is it necessary to clone camera_request? Can it be passed by reference?
            let send_futures: Vec<_> = offers
                .iter()
                .map(|offer| offer.snd.send(Ok(camera_request.clone())))
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
                rx: mpsc::Receiver<Snapshot>,
                vec: &mut Vec<Snapshot>,
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
                let mut channels_hm = self.snapshot_channels.write();
                channels_hm.remove(&snapshot_id)
            };

            if camera_responses.len() > 0 {
                let response = ServerSnapshotResponse {
                    snapshot_id,
                    messages: camera_responses,
                };
                Ok(Response::new(response))
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

    async fn get_snapshot_cameras(
        &self,
        request: Request<ServerSnapshotRequest>,
    ) -> Result<Response<SnapshotCamerasResponse>, Status> {
        let ServerSnapshotRequest { group_name } = request.into_inner();
        let all_offers = self.snapshot_offers.read().await;
        let group_offers = all_offers
            .get(&group_name)
            .ok_or(Status::unavailable(format!(
                "No outstanding snapshot offers for group {}",
                group_name
            )))?;

        let cameras = group_offers
            .values()
            .map(|offer| offer.camera.clone())
            .collect();
        let message = SnapshotCamerasResponse { cameras };

        Ok(Response::new(message))
    }
    */

    // Camera

    async fn hello(&self, request: Request<CameraInfo>) -> Result<Response<SessionToken>, Status> {
        let info = request.into_inner();
        self.cameras_tx.send(info).await.map_err(|_| {
            Status::internal("Camera channel was closed")
        })?;

        let token = SessionToken::new();

        Ok(Response::new(token))
    }

    async fn camera_control(
        &self,
        request: Request<SessionToken>,
    ) -> Result<Response<Self::CameraControlStream>, Status> {
        todo!()
    }

    async fn camera_data_sink(
        &self,
        request: Request<tonic::Streaming<CameraMessage>>,
    ) -> Result<Response<SendDataSuccess>, Status> {
        let mut stream = request.into_inner();

        // First message in stream must be a command token.
        if let Some(Ok(CameraMessage {
            msg: Some(camera_message::Msg::Token(token)),
        })) = stream.next().await
        {
            // TODO: Move to self.get_data_channel(&token) function
            let tx = {
                let hm = self.data_channels.read().await;
                if let Some(_tx) = hm.get(&token) {
                    _tx.clone()
                } else {
                    return Err(Status::failed_precondition(
                        "No data channel found for given CommandToken.",
                    ));
                }
            };

            // Send a Begin message to indicate that the CommandToken has been received,
            // and streaming data may follow.
            tx.send(CommandResponseMessage::Begin)
                .await
                .or(Err(Status::failed_precondition("Data channel closed.")))?;

            // TODO: What would Some(Err(_)) mean here? And how to deal with it?
            while let Some(Ok(CameraMessage {
                msg: Some(camera_message::Msg::Response(command_response)),
            })) = stream.next().await
            {
                // TODO: This could be made more efficient, possibly by using tokio::spawn.
                // It's not necessary to wait for sending to complete
                // before retrieving the next value from the stream.

                // But I couldn't get it to work,
                // related to https://github.com/rust-lang/rust/issues/78633

                // Listen for incoming messages in stream
                tx.send(CommandResponseMessage::Data(command_response))
                    .await
                    .or(Err(Status::failed_precondition("Data channel closed.")))?;
            }

            Ok(Response::new(SendDataSuccess {}))
        } else {
            Err(Status::invalid_argument(
                "Stream must begin with a CommandToken.",
            ))
        }
    }

    // Admin

    async fn list_groups(
        &self,
        request: Request<ListGroupsRequest>,
    ) -> Result<Response<ListGroupsResponse>, Status> {
        todo!()
    }

    async fn list_cameras(
        &self,
        request: Request<ListCamerasRequest>,
    ) -> Result<Response<ListCamerasResponse>, Status> {
        todo!()
    }

    async fn get_camera_info(
        &self,
        request: Request<CameraIdentifier>,
    ) -> Result<Response<CameraInfo>, Status> {
        todo!()
    }

    async fn stream_control(
        &self,
        request: Request<StreamControlRequest>,
    ) -> Result<Response<StreamStatus>, Status> {
        todo!()
    }

    async fn take_snapshots(
        &self,
        request: Request<ServerSnapshotRequest>,
    ) -> Result<Response<ServerSnapshotResponse>, Status> {
        todo!()
    }

    async fn get_current(
        &self,
        request: Request<CameraIdentifier>,
    ) -> Result<Response<ServerSnapshotResponse>, Status> {
        todo!()
    }

    async fn calibrate(
        &self,
        request: Request<CalibrationRequest>,
    ) -> Result<Response<CalibrationResponse>, Status> {
        match request.into_inner() {
            CalibrationRequest {
                which_camera: Some(which_camera),
                command: Some(calibrate_command),
            } => {
                let control_command = camera_control_command::Command::Calibrate(calibrate_command);
                let execution_futures = self.execute_command(which_camera, control_command).await;
                let execution_results = join_all(execution_futures).await;

                let mut calibration_states = Vec::new();
                for (camera, stream) in execution_results {
                    // https://stackoverflow.com/a/64007300/4228052
                    pin_mut!(stream);

                    // TODO: Is this inefficient to await in a loop?
                    // Couldn't get it working w/ join_all
                    let maybe_response = stream.next().await;
                    // TODO: Don't silently ignore errors.
                    // Should tell caller which cameras failed.
                    if let Ok(calibration_state) =
                        construct_calibration_state(camera, maybe_response)
                    {
                        calibration_states.push(calibration_state);
                    }
                }

                Ok(Response::new(CalibrationResponse {
                    states: calibration_states,
                }))
            }
            CalibrationRequest { command: None, .. } => {
                Err(Status::invalid_argument("Missing field: 'command'"))
            }
            CalibrationRequest {
                which_camera: None, ..
            } => Err(Status::invalid_argument("Missing field: 'which_camera'")),
        }
    }

    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        if let PingRequest {
            which_camera: Some(which_camera),
            timeout: maybe_timeout,
        } = request.into_inner()
        {
            // Ping some cameras
            let command = camera_control_command::Command::Ping(Ping {});
            let send_time = Instant::now();

            let response_stream_futures = self
                .execute_command(which_camera, command)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let num_pings = response_stream_futures.len();
            // TODO: Await responses in parallel w/ timeout
            let (ping_tx, ping_rx) = mpsc::channel(num_pings);
            let mapped_futures: Vec<_> = response_stream_futures
                .into_iter()
                .map(|future| {
                    // Chain future
                    future.then(|(camera, _)| async {
                        // Ignore the contents of the message stream - ping only sends a single message.
                        // Once we receive the stream, then the ping has returned.
                        let receive_time = Instant::now();
                        let elapsed = receive_time - send_time;
                        let ping_results = PingResults {
                            response_time: Some(elapsed.into()),
                            which_camera: None,
                        };
                        ping_tx.send(ping_results).await;
                    })
                })
                .collect();

            // TODO: Set default timeout somewhere else?
            let timeout = maybe_timeout
                .and_then(|t| t.try_into().ok())
                .unwrap_or(Duration::from_secs(5));

            let mut results_buf = Vec::new();
            let watcher = ChannelWatcher::new(ping_rx, &mut results_buf, num_pings);

            // Return results when all have been received
            // or timeout is reached, whichever comes first.
            let results = select! {
                _ = watcher => results_buf,
                _ = tokio::time::sleep(timeout) => results_buf
            };

            Ok(Response::new(PingResponse { results }))
        } else {
            // Don't ping any cameras, just respond immediately
            return Ok(Response::new(PingResponse::default()));
        }
    }

    // A la carte

    async fn triangulate(
        &self,
        request: Request<tonic::Streaming<TriangulationRequest>>,
    ) -> Result<Response<TriangulationResponse>, Status> {
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
                    Err(Status::invalid_argument(
                        "All snapshots must currently have the same number of poses.",
                    ))?;
                }
            }
            // Save camera, too
            if let Some(camera) = camera {
                cameras.push(camera.into());
            } else {
                Err(Status::invalid_argument("All camera data must be present."))?;
            }
            // Increment counter
            i += 1;
        }

        // Convert CameraInfo objects to Camera Matrices
        let camera_matrices = cameras
            .iter()
            .map(|camera| -> anyhow::Result<_> {
                Ok(triangulator::calculate_camera_matrix(
                    camera
                        .calibration
                        .as_ref()
                        .ok_or(MissingField::Calibration)?,
                )?)
            })
            .collect::<Result<Vec<_>, _>>()
            .or(Err(Status::invalid_argument(
                "Not all required camera info was provided.",
            )))?;

        let poses3d = poses_by_subject
            .into_iter()
            .map(|poses| {
                triangulator::triangulate_from_poses_and_camera_matrices(poses, &camera_matrices)
            })
            .collect::<Result<Vec<_>, HubError>>()
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let response = TriangulationResponse { poses: poses3d };

        log::info!("Finished triangulate request");

        Ok(Response::new(response))
    }

    async fn bundle_adjustment(
        &self,
        request: Request<BundleAdjustmentRequest>,
    ) -> Result<Response<BundleAdjustmentResponse>, Status> {
        let message = request.into_inner();
        let nkeypoints = 17;
        let views = message.views;
        let nviews = views.len();
        let nposes = if nviews > 0 { views[0].poses.len() } else { 0 };
        log::info!(
            "Got bundle adjustment request with {} views and {} poses",
            nviews,
            nposes
        );
        log::info!("BA opts: {:#?}", message.options);

        // Combine all poses into single matrix
        let npoints_total = nkeypoints * nposes;
        let mut xs = Vec::with_capacity(nviews);
        let mut x3d = Matrix3xX::zeros(npoints_total);
        let mut ks = Vec::with_capacity(nviews);
        let mut rs = Vec::with_capacity(nviews);
        let mut ts = Vec::with_capacity(nviews);

        // Collect initial guess
        let mut original_scores = Vec::with_capacity(npoints_total);
        for (k, initial_pose) in message.initial_poses.into_iter().enumerate() {
            let (spoints, _): (Vec<SPoint3>, f64) = initial_pose.into();
            for (h, (point, score)) in spoints.iter().enumerate() {
                let j = nkeypoints * k + h;
                let col = Vector3::new(point.x, point.y, point.z);
                original_scores.push(score.clone());
                x3d.set_column(j, &col);
            }
        }

        let mut orig_cameras = Vec::with_capacity(nviews);
        for view in views {
            // Collect keypoint observations from this camera
            let mut x = Matrix2xX::zeros(npoints_total);
            for (k, pose) in view.poses.into_iter().enumerate() {
                let (spoints, _): (Vec<SPoint2>, f64) = pose
                    .try_into()
                    .map_err(|e: MissingField| Status::invalid_argument(e.to_string()))?;
                for (h, (point, _score)) in spoints.iter().enumerate() {
                    let j = nkeypoints * k + h;
                    // TODO: Use score
                    let col = Vector2::new(point.x, point.y);
                    x.set_column(j, &col);
                }
            }
            xs.push(x);

            let camera = view.camera.ok_or_else(|| {
                Status::invalid_argument("Each view must have camera data")
            })?;
            let calibration = camera.calibration.as_ref().ok_or_else(|| {
                Status::invalid_argument("Camera missing calibration")
            })?;
            let intrinsics = calibration.intrinsics.as_ref().ok_or_else(|| {
                Status::invalid_argument("Calibration missing intrinsics")
            })?;
            let extrinsics = calibration.extrinsics.as_ref().ok_or_else(|| {
                Status::invalid_argument("Calibration missing extrinsics")
            })?;
            let k = Matrix3::from_row_slice(&intrinsics.camera_matrix);
            let c = Matrix4::from_row_slice(&extrinsics.view_matrix);
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

        let result = ceres_bundle_adjustment(
            &xs,
            &mut ks,
            &mut ts,
            &mut rs,
            &mut x3d,
            message.options,
        )
        .map_err(|e| Status::internal(e.to_string()))?;

        if !result {
            log::error!("Bundle adjustment failed");
            return Err(Status::internal("Bundle adjustment failed"));
        }

        // Unpack results back into protobuf types
        let mut cameras = Vec::with_capacity(nviews);
        for (i, orig_camera) in orig_cameras.into_iter().enumerate() {
            let CameraIdentifier {
                group_name,
                camera_name,
            } = orig_camera
                .which_camera
                .ok_or(Status::invalid_argument("Missing `which_camera` field"))?;

            // TODO: Get new distortion - currently just using original distortion
            let distortion = orig_camera
                .calibration
                .and_then(|cal| cal.intrinsics)
                .map(|int| int.distortion)
                .unwrap_or_default();

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
                which_camera: Some(CameraIdentifier {
                    camera_name,
                    group_name,
                }),
                calibration: Some(CalibrationParameters {
                    extrinsics: Some(CameraExtrinsics { view_matrix }),
                    intrinsics: Some(CameraIntrinsics {
                        camera_matrix,
                        distortion,
                        rms_error: 0.0, // Not sure what to do with this
                    }),
                }),
            };
            cameras.push(camera);
        }

        // Unpack poses
        let use_orig_scores = original_scores.len() > 0;
        let mut poses = Vec::with_capacity(nposes);
        for k in 0..nposes {
            let mut spoints = Vec::with_capacity(nkeypoints);
            for h in 0..nkeypoints {
                let j = nkeypoints * k + h;
                let col = x3d.column(j);
                let point = Point3::from_slice(col.as_slice());
                // Use scores from initial guess if provided
                let score = if use_orig_scores {
                    original_scores[j]
                } else {
                    1.0
                };
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
        log::info!("Bundle adjustment completed successfully.");
        Ok(Response::new(response))
    }
}

fn construct_calibration_state(
    camera: CameraUniqueIdentifier,
    maybe_response: Option<CommandResponse>,
) -> Result<CalibrationState, CalibrationResponseError> {
    match maybe_response {
        Some(CommandResponse {
            response: Some(command_response::Response::Calibration(params)),
        }) => Ok(CalibrationState {
            which_camera: Some(camera.into()),
            calibration: Some(params),
        }),
        Some(CommandResponse {
            response: Some(wrong),
        }) => Err(CalibrationResponseError::WrongResponseType(wrong)),
        Some(CommandResponse { response: None }) => {
            Err(CalibrationResponseError::EmptyCommandResponse)
        }
        None => Err(CalibrationResponseError::NoCommandResponse),
    }
}

#[derive(Error, Debug)]
enum CalibrationResponseError {
    #[error("Expected Calibration, received: {0:?}")]
    WrongResponseType(command_response::Response),
    #[error("CommandResponse contained no response")]
    EmptyCommandResponse,
    #[error("No command response (token received, then stream closed.)")]
    NoCommandResponse,
}

struct CameraSession {
    camera: CameraUniqueIdentifier,
    token: SessionToken,
}

struct CommandResponseStream {
    camera: CameraUniqueIdentifier,
    rx: mpsc::Receiver<CommandResponseMessage>,
}

async fn send_control_command(
    command: camera_control_command::Command,
    channels: Vec<mpsc::Sender<CameraControlCommand>>,
    tokens: Vec<CommandToken>,
) -> Result<(), HubError> {
    if channels.len() != tokens.len() {
        return Err(InvalidInput::Message(format!(
            "send_control_command: channels.len() {} != tokens.len() {}",
            channels.len(),
            tokens.len()
        ))
        .into());
    }
    let futures = channels.into_iter().zip(tokens).map(|(tx, token)| {
        let control_command = CameraControlCommand {
            token: Some(token),
            command: Some(command.clone()),
        };
        async move { tx.send(control_command).await }
    });
    for fut in join_all(futures).await {
        fut.map_err(|_| crate::errors::ChannelError::SendClosed)?;
    }
    Ok(())
}

impl HubServer {
    /// Return a vector of all tokens that match the given identifier
    /// - If `group_name` and `camera_name` are specified: match one camera
    /// - If only `group_name` is specified: match all cameras in a group
    /// - If neither is specified: match all cameras
    async fn get_sessions(&self, which_camera: CameraIdentifier) -> Vec<CameraSession> {
        match which_camera {
            CameraIdentifier {
                group_name,
                camera_name,
            } if group_name == "" && camera_name == "" => {
                // Match all cameras
                let mut sessions = Vec::new();

                let tokens_hm = self.session_tokens.read().await;
                for (group_name, group_hm_lock) in tokens_hm.iter() {
                    let group_hm = group_hm_lock.read().await;
                    for (camera_name, token) in group_hm.iter() {
                        let camera = CameraUniqueIdentifier {
                            group_name: group_name.clone(),
                            camera_name: camera_name.clone(),
                        };
                        let session = CameraSession {
                            camera,
                            token: token.clone(),
                        };
                        sessions.push(session);
                    }
                }

                sessions
            }
            CameraIdentifier {
                group_name,
                camera_name,
            } if camera_name == "" => {
                // Match one group
                let tokens_hm = self.session_tokens.read().await;
                if let Some(group_hm_lock) = tokens_hm.get(&group_name) {
                    let group_hm = group_hm_lock.read().await;
                    let mut sessions = Vec::new();
                    for (camera_name, token) in group_hm.iter() {
                        let camera = CameraUniqueIdentifier {
                            group_name: group_name.clone(),
                            camera_name: camera_name.clone(),
                        };
                        let session = CameraSession {
                            camera,
                            token: token.clone(),
                        };
                        sessions.push(session);
                    }

                    sessions
                } else {
                    // ERROR: Group doesn't exist
                    // TODO: Return result
                    Vec::new()
                }
            }
            CameraIdentifier {
                group_name,
                camera_name,
            } => {
                // Match one camera
                let tokens_hm = self.session_tokens.read().await;
                if let Some(group_hm_lock) = tokens_hm.get(&group_name) {
                    let group_hm = group_hm_lock.read().await;
                    if let Some(token) = group_hm.get(&camera_name) {
                        let camera = CameraUniqueIdentifier {
                            group_name,
                            camera_name,
                        };
                        vec![CameraSession {
                            camera,
                            token: token.clone(),
                        }]
                    } else {
                        // ERROR: Camera doesn't exist
                        // TODO: Return result
                        vec![]
                    }
                } else {
                    // ERROR: Group doesn't exist
                    // TODO: Return result
                    vec![]
                }
            }
        }
    }

    async fn get_control_channels(
        &self,
        tokens: &[SessionToken],
    ) -> Vec<mpsc::Sender<CameraControlCommand>> {
        let channels_hm = self.control_channels.read().await;
        tokens
            .iter()
            .filter_map(|token| {
                channels_hm.get(&token).or_else(|| {
                    log::error!("No control channel for {:?}", token);
                    None
                })
            })
            .cloned()
            .collect()
    }

    /// Create data channels for command responses.
    /// Store `Sender`s for later lookup, and return `Receiver`s immediately.
    async fn create_data_channels(
        &self,
        tokens: Vec<CommandToken>,
    ) -> Vec<mpsc::Receiver<CommandResponseMessage>> {
        let mut rxs = Vec::with_capacity(tokens.len());
        let mut rx_hm = self.data_channels.write();
        for token in tokens {
            let (tx, rx) = mpsc::channel(10);
            rxs.push(rx);
            rx_hm.insert(token, tx);
        }
        rxs
    }

    async fn retrieve_command_responses(&self) {
        // TODO: What is this supposed to do? Probably useless.
        todo!()
    }

    /// Send command & retrieve results
    async fn execute_command(
        &self,
        which_camera: CameraIdentifier,
        command: camera_control_command::Command,
    ) -> Vec<
        impl Future<
            Output = (
                CameraUniqueIdentifier,
                impl Stream<Item = CommandResponse> + Send,
            ),
        >,
    > {
        // Look up & unpack active camera sessions
        // TODO: filter_map & or_else(log error) in get_sessions
        let sessions = self.get_sessions(which_camera).await;
        let (cameras, session_tokens): (Vec<_>, Vec<_>) = sessions
            .into_iter()
            .map(|session| (session.camera, session.token))
            .unzip();

        // Look up control channels for sessions
        let control_channels = self.get_control_channels(&session_tokens).await;

        // Create command tokens
        let command_tokens: Vec<_> = control_channels
            .iter()
            .map(|_| CommandToken::new())
            .collect();

        // Get data channels
        let rxs = self.create_data_channels(command_tokens.clone()).await;

        // Send command over channels
        send_control_command(command, control_channels, command_tokens).await?;

        // Return access to incoming data, labeled by camera
        let response_streams: Vec<_> = cameras
            .into_iter()
            .zip(rxs)
            .map(|(camera, rx)| CommandResponseStream { camera, rx })
            .collect();

        // Receive Begin message before fulfilling future for CommandResponseStream
        let stream_futures: Vec<_> = response_streams
            .into_iter()
            .map(|CommandResponseStream { camera, rx }| {
                let stream_fut = ReceiverStream::new(rx).into_future();

                stream_fut.map(|(head, tail)| {
                    // Wait for first message indicating that the stream has started.
                    // Box must be pinned to use methods that cosume its contents
                    // See https://stackoverflow.com/a/61265318/4228052
                    let tail_box: Pin<Box<dyn Stream<Item = CommandResponseMessage> + Send>> =
                        match head {
                            Some(CommandResponseMessage::Begin) => Box::pin(tail),
                            Some(msg @ CommandResponseMessage::Data(_)) => {
                                // We should always receive Begin first, but if we receive Data first,
                                // then just wrap it in a future, and prepend it to the head of the stream.
                                let prefix = futures::stream::once(async { msg });
                                Box::pin(prefix.chain(tail))
                            }
                            None => Box::pin(tail), // Empty tail
                        };

                    // Once the Begin message is received, immediately return the stream
                    // that will contain the actual responses.
                    // We should only be receiving Data after the first Begin message,
                    // but drop Begins silently if they are received for some reason.
                    let inner_stream = tail_box.filter_map(get_inner_command_response);
                    (camera, inner_stream)
                })
            })
            .collect();

        Ok(stream_futures)
    }
}

async fn get_inner_command_response(message: CommandResponseMessage) -> Option<CommandResponse> {
    match message {
        CommandResponseMessage::Data(response) => Some(response),
        _ => None,
    }
}

struct ChannelWatcher<'a, T> {
    rx: Receiver<T>,
    buf: &'a mut Vec<T>,
    target: usize,
}

impl<'a, T> ChannelWatcher<'a, T> {
    pub fn new(rx: Receiver<T>, buf: &'a mut Vec<T>, target: usize) -> Self {
        // let buf = Vec::with_capacity(target);
        Self { rx, buf, target }
    }
}

/// Ready when the number of items received reaches `target`
impl<'a, T> Future for ChannelWatcher<'a, T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        while self.buf.len() < self.target {
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(thing)) => {
                    // Save incoming data
                    self.buf.push(thing);
                }
                Poll::Ready(None) => {
                    // Nothing else coming; return now
                    return Poll::Ready(());
                }
                Poll::Pending => {
                    // Still waiting
                    return Poll::Pending;
                }
            }
        }

        // Target reached
        Poll::Ready(())
    }
}

// TODO: Use builtin gRPC errors?
// Or gRPC extended error syntax?
#[derive(Debug)]
enum HubServerError {
    InvalidRequest { message: String },
    SessionTokenMissing,
    CommandTokenMissing,
}

pub struct GrpcConfig {
    addr: SocketAddr,
}

impl GrpcConfig {
    pub fn new(ip: &str, port: u16) -> Result<Self, ConfigError> {
        Ok(Self {
            addr: format!("{}:{}", ip, port).parse()?,
        })
    }

    pub fn try_default() -> Result<Self, ConfigError> {
        Self::new("0.0.0.0", 50051)
    }
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self::try_default().expect("default gRPC config 0.0.0.0:50051 must be valid")
    }
}

pub struct GrpcServer {
    config: GrpcConfig,
    cameras_tx: mpsc::Sender<CameraInfo>,
    snapshots_tx: mpsc::Sender<Snapshot>,
}

impl GrpcServer {
    pub fn new(
        config: GrpcConfig,
        cameras_tx: mpsc::Sender<CameraInfo>,
        snapshots_tx: mpsc::Sender<Snapshot>,
    ) -> Self {
        Self {
            config,
            cameras_tx,
            snapshots_tx,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        log::info!("PoseNet Hub gRPC service listening on {}", self.config.addr);

        let hub_server = HubServer {
            cameras_tx: self.cameras_tx,
            snapshots_tx: self.snapshots_tx,
            session_tokens: RwLock::new(HashMap::new()),
            stream_cache: RwLock::new(HashMap::new()),
            control_channels: RwLock::new(HashMap::new()),
            data_channels: RwLock::new(HashMap::new()),
        };

        Server::builder()
            .add_service(HubServiceServer::new(hub_server))
            .serve(self.config.addr)
            .await?;

        Ok(())
    }
}

// TODO: Move these tests elsewhere?
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_hello() {
        assert!(async { true }.await);
        todo!()
    }

    #[tokio::test]
    async fn test_camera_control() {
        todo!()
    }

    #[tokio::test]
    async fn test_camera_data_sink() {
        todo!()
    }

    // Admin

    #[tokio::test]
    async fn test_list_groups() {
        todo!()
    }

    #[tokio::test]
    async fn test_list_cameras() {
        todo!()
    }

    #[tokio::test]
    async fn test_get_camera_info() {
        todo!()
    }

    #[tokio::test]
    async fn test_stream_control() {
        todo!()
    }

    #[tokio::test]
    async fn test_take_snapshots() {
        todo!()
    }

    #[tokio::test]
    async fn test_get_current() {
        todo!()
    }

    #[tokio::test]
    async fn test_calibrate() {
        todo!()
    }

    #[tokio::test]
    async fn test_ping() {
        todo!()
    }

    // A la carte

    #[tokio::test]
    async fn test_triangulate() {
        todo!()
    }

    #[tokio::test]
    async fn test_bundle_adjustment() {
        todo!()
    }
}
