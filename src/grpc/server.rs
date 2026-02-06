use futures::future::join_all;
use futures::{pin_mut, Future, FutureExt, Stream, StreamExt};
use nalgebra::{Matrix2xX, Matrix3, Matrix3xX, Matrix4, Point3, Vector2, Vector3};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant, SystemTime};
use std::{net::SocketAddr, pin::Pin};
use thiserror::Error;
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use tonic::{transport::Server, Request, Response, Status};
use uuid::Uuid;

use crate::errors::{ConfigError, HubError, InvalidInput, MissingField};
use crate::openmvg::openmvg::ceres_bundle_adjustment;
use crate::recording::RecordingState;

use super::proto::hub_service_server::{HubService, HubServiceServer};
use super::proto::*;

pub struct HubServer {
    cameras_tx: mpsc::UnboundedSender<CameraInfo>,
    snapshots_tx: mpsc::UnboundedSender<Snapshot>,
    /// Optional recording state; when set, snapshots are teed to the recording file.
    recording_state: Option<Arc<RecordingState>>,
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
    /// Latest streamed snapshots (Arc to avoid cloning ~900KB image on every frame).
    /// Indexed by group name, then camera name.
    stream_cache: Arc<RwLock<HashMap<String, RwLock<HashMap<String, Arc<Snapshot>>>>>>,
    /// Which groups are currently streaming and their params
    stream_state: Arc<RwLock<HashMap<String, StreamParameters>>>,
    /// Active camera control channels, indexed by session token
    /// TODO: Use token data as key, not whole token?
    control_channels: RwLock<HashMap<SessionToken, mpsc::Sender<CameraControlCommand>>>,
    /// Pending control stream receivers; one per session, consumed when camera calls CameraControl
    pending_control_rx: RwLock<HashMap<SessionToken, mpsc::Receiver<CameraControlCommand>>>,
    /// Channels to route incoming data from cameras, indexed by command token.
    /// Unbounded so gRPC camera_data_sink never blocks on slow stream_cache consumers.
    data_channels: RwLock<HashMap<CommandToken, mpsc::UnboundedSender<CommandResponseMessage>>>,
    /// Stored CameraInfo per camera (from Hello), for GetCameraInfo and TakeSnapshots
    camera_info: RwLock<HashMap<CameraIdentifier, CameraInfo>>,
}

impl HubServer {
    /// Create a new HubServer with empty session/cache state. Caller keeps cameras_tx/snapshots_tx.
    /// If recording_state is Some, incoming streamed snapshots are teed to the recording file when recording is active.
    pub fn new(
        cameras_tx: mpsc::UnboundedSender<CameraInfo>,
        snapshots_tx: mpsc::UnboundedSender<Snapshot>,
        recording_state: Option<Arc<RecordingState>>,
    ) -> Self {
        let stream_cache = Arc::new(RwLock::new(HashMap::new()));
        let stream_state = Arc::new(RwLock::new(HashMap::new()));
        Self {
            cameras_tx,
            snapshots_tx,
            recording_state,
            session_tokens: RwLock::new(HashMap::new()),
            stream_cache,
            stream_state,
            control_channels: RwLock::new(HashMap::new()),
            pending_control_rx: RwLock::new(HashMap::new()),
            data_channels: RwLock::new(HashMap::new()),
            camera_info: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the latest cached snapshot for the given group and camera, if any.
    /// Used by the HTTP MJPEG server to stream camera video.
    pub fn get_latest_snapshot(
        &self,
        group_name: &str,
        camera_name: &str,
    ) -> Option<Arc<Snapshot>> {
        let cache = self.stream_cache.read();
        cache
            .get(group_name)
            .and_then(|group_hm| group_hm.read().get(camera_name).cloned())
    }
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
                let channels_hm = self.snapshot_channels.read();

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
            let all_offers = self.snapshot_offers.read();

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
                    timestamp: None,
                    snapshots: camera_responses,
                    poses3d: vec![],
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
        let all_offers = self.snapshot_offers.read();
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
        let which_camera = info
            .which_camera
            .clone()
            .ok_or_else(|| Status::invalid_argument("Hello requires which_camera"))?;
        let group_name = which_camera.group_name.clone();
        let camera_name = which_camera.camera_name.clone();

        let token = SessionToken::new();

        // Register session and control channel
        {
            let mut tokens_hm = self.session_tokens.write();
            let group_hm = tokens_hm
                .entry(group_name.clone())
                .or_insert_with(|| RwLock::new(HashMap::new()));
            group_hm.write().insert(camera_name.clone(), token.clone());
        }
        let (ctrl_tx, ctrl_rx) = mpsc::channel(32);
        self.control_channels.write().insert(token.clone(), ctrl_tx);
        self.pending_control_rx
            .write()
            .insert(token.clone(), ctrl_rx);
        self.camera_info
            .write()
            .insert(which_camera.clone(), info.clone());
        self.cameras_tx
            .send(info)
            .map_err(|_| Status::internal("Camera channel was closed"))?;

        Ok(Response::new(token))
    }

    async fn camera_control(
        &self,
        request: Request<SessionToken>,
    ) -> Result<Response<Self::CameraControlStream>, Status> {
        let token = request.into_inner();
        let mut rx = self
            .pending_control_rx
            .write()
            .remove(&token)
            .ok_or_else(|| Status::failed_precondition("Unknown or already used session token"))?;
        let (wrap_tx, wrap_rx) = mpsc::channel(32);
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                let _ = wrap_tx.send(Ok(cmd)).await;
            }
        });
        Ok(Response::new(ReceiverStream::new(wrap_rx)))
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
                let hm = self.data_channels.read();
                if let Some(_tx) = hm.get(&token) {
                    _tx.clone()
                } else {
                    return Err(Status::failed_precondition(
                        "No data channel found for given CommandToken.",
                    ));
                }
            };

            // Send a Begin message to indicate that the CommandToken has been received,
            // and streaming data may follow. UnboundedSender::send is non-blocking.
            tx.send(CommandResponseMessage::Begin)
                .or(Err(Status::failed_precondition("Data channel closed.")))?;

            // Unbounded channel: we never block here, so the gRPC stream can drain at full speed.
            while let Some(Ok(CameraMessage {
                msg: Some(camera_message::Msg::Response(command_response)),
            })) = stream.next().await
            {
                tx.send(CommandResponseMessage::Data(command_response))
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
        _request: Request<ListGroupsRequest>,
    ) -> Result<Response<ListGroupsResponse>, Status> {
        let group_names: Vec<String> = self.session_tokens.read().keys().cloned().collect();
        Ok(Response::new(ListGroupsResponse { group_names }))
    }

    async fn list_cameras(
        &self,
        request: Request<ListCamerasRequest>,
    ) -> Result<Response<ListCamerasResponse>, Status> {
        let group_name = request.into_inner().group_name;
        let camera_names = self
            .session_tokens
            .read()
            .get(&group_name)
            .map(|group_hm| group_hm.read().keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        Ok(Response::new(ListCamerasResponse { camera_names }))
    }

    async fn get_camera_info(
        &self,
        request: Request<CameraIdentifier>,
    ) -> Result<Response<CameraInfo>, Status> {
        let which_camera = request.into_inner();
        let info = self
            .camera_info
            .read()
            .get(&which_camera)
            .cloned()
            .ok_or_else(|| Status::not_found("Camera not found"))?;
        Ok(Response::new(info))
    }

    async fn stream_control(
        &self,
        request: Request<StreamControlRequest>,
    ) -> Result<Response<StreamStatus>, Status> {
        let req = request.into_inner();
        let group_name = req.group_name;
        match req.command {
            Some(stream_control_request::Command::StartStreaming(start)) => {
                let params = start
                    .params
                    .ok_or_else(|| Status::invalid_argument("StartStreaming requires params"))?;
                let which_camera = CameraIdentifier {
                    group_name: group_name.clone(),
                    camera_name: String::new(),
                };
                let command =
                    camera_control_command::Command::StartStreaming(StartStreamingCommand {
                        params: Some(params.clone()),
                    });
                let execution_futures = self
                    .execute_command(which_camera, command)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
                self.stream_state
                    .write()
                    .insert(group_name.clone(), params.clone());
                let execution_results = join_all(execution_futures).await;
                let stream_cache = self.stream_cache.clone();
                let snapshots_tx = self.snapshots_tx.clone();
                let stream_state = self.stream_state.clone();
                let recording_state = self.recording_state.clone();
                for (_camera, stream) in execution_results {
                    let group_name = group_name.clone();
                    let stream_cache = stream_cache.clone();
                    let snapshots_tx = snapshots_tx.clone();
                    let stream_state = stream_state.clone();
                    let recording_state = recording_state.clone();
                    tokio::spawn(async move {
                        pin_mut!(stream);
                        let mut frames_cached: u64 = 0;
                        while let Some(response) = stream.next().await {
                            if let Some(command_response::Response::Snapshot(snapshot)) =
                                response.response
                            {
                                let (g, c) = snapshot
                                    .which_camera
                                    .as_ref()
                                    .map(|id| (id.group_name.clone(), id.camera_name.clone()))
                                    .unwrap_or_else(|| (String::new(), String::new()));
                                if !g.is_empty() && !c.is_empty() {
                                    let arc = Arc::new(snapshot);
                                    let camera_name = c.clone();
                                    stream_cache
                                        .write()
                                        .entry(g.clone())
                                        .or_insert_with(|| RwLock::new(HashMap::new()))
                                        .write()
                                        .insert(c, Arc::clone(&arc));
                                    frames_cached += 1;
                                    if frames_cached % 30 == 0 {
                                        log::debug!(
                                            "stream_cache: {} frames cached for {}/{}",
                                            frames_cached,
                                            g,
                                            camera_name
                                        );
                                    }
                                    let snapshot_for_tx = Arc::try_unwrap(arc)
                                        .unwrap_or_else(|a| (*a).clone());
                                    if let Some(ref rec) = recording_state {
                                        rec.tee_snapshot(&snapshot_for_tx).await;
                                    }
                                    let _ = snapshots_tx.send(snapshot_for_tx);
                                }
                            }
                        }
                        // Stream ended; remove this group from stream_state if no other refs
                        stream_state.write().remove(&group_name);
                    });
                }
                Ok(Response::new(StreamStatus {
                    is_streaming: true,
                    params: Some(params),
                }))
            }
            Some(stream_control_request::Command::StopStreaming(_)) => {
                let which_camera = CameraIdentifier {
                    group_name: group_name.clone(),
                    camera_name: String::new(),
                };
                let command =
                    camera_control_command::Command::StopStreaming(StopStreamingCommand {});
                let _ = self.execute_command(which_camera, command).await;
                self.stream_state.write().remove(&group_name);
                Ok(Response::new(StreamStatus {
                    is_streaming: false,
                    params: None,
                }))
            }
            None => Err(Status::invalid_argument(
                "StreamControlRequest requires command",
            )),
        }
    }

    async fn take_snapshots(
        &self,
        request: Request<ServerSnapshotRequest>,
    ) -> Result<Response<ServerSnapshotResponse>, Status> {
        let req = request.into_inner();
        let which_camera = req
            .which_camera
            .ok_or_else(|| Status::invalid_argument("TakeSnapshots requires which_camera"))?;
        let want_pose3d = req.want_pose3d;
        let command = camera_control_command::Command::TakeSnapshot(TakeSnapshotCommand {
            params: req.params,
        });
        let execution_futures = self
            .execute_command(which_camera.clone(), command)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let execution_results = join_all(execution_futures).await;
        let timeout = Duration::from_secs(5);
        let mut snapshots = Vec::new();
        for (_camera, stream) in execution_results {
            pin_mut!(stream);
            let first = tokio::time::timeout(timeout, stream.next()).await;
            if let Ok(Some(response)) = first {
                if let Some(command_response::Response::Snapshot(snapshot)) = response.response {
                    snapshots.push(snapshot);
                }
            }
        }
        let snapshot_id = Uuid::new_v4().to_string();
        let timestamp = SystemTime::now();
        let poses3d = if want_pose3d && snapshots.len() >= 2 {
            let camera_info_hm = self.camera_info.read();
            let matrices: Vec<_> = snapshots
                .iter()
                .filter_map(|s| s.which_camera.as_ref())
                .filter_map(|id| camera_info_hm.get(id))
                .filter_map(|info| info.calibration.as_ref())
                .filter_map(|cal| crate::triangulator::calculate_camera_matrix(cal).ok())
                .collect();
            if matrices.len() != snapshots.len() {
                vec![]
            } else {
                let groups = crate::triangulator::match_poses(
                    &snapshots,
                    &matrices,
                    crate::triangulator::POSE_MATCHING_REPROJECTION_THRESHOLD_PX,
                );
                crate::triangulator::triangulate_matched_groups(&groups, &matrices)
                    .into_iter()
                    .filter_map(Result::ok)
                    .collect()
            }
        } else {
            vec![]
        };
        Ok(Response::new(ServerSnapshotResponse {
            snapshot_id,
            timestamp: Some(timestamp.into()),
            snapshots,
            poses3d,
        }))
    }

    async fn get_current(
        &self,
        request: Request<CameraIdentifier>,
    ) -> Result<Response<ServerSnapshotResponse>, Status> {
        let which_camera = request.into_inner();
        let group_name = which_camera.group_name;
        let camera_name = which_camera.camera_name;
        let cache = self.stream_cache.read();
        let snapshots: Vec<Snapshot> = if camera_name.is_empty() {
            cache
                .get(&group_name)
                .map(|group_hm| {
                    group_hm
                        .read()
                        .values()
                        .map(|arc| arc.as_ref().clone())
                        .collect::<Vec<Snapshot>>()
                })
                .unwrap_or_default()
        } else {
            cache
                .get(&group_name)
                .and_then(|group_hm| group_hm.read().get(&camera_name).cloned())
                .map(|s| vec![(*s).clone()])
                .unwrap_or_default()
        };
        if snapshots.is_empty() {
            return Err(Status::not_found(
                "No current snapshot in cache for requested camera(s)",
            ));
        }
        Ok(Response::new(ServerSnapshotResponse {
            snapshot_id: "current".to_string(),
            timestamp: snapshots
                .first()
                .and_then(|s| s.timestamp.clone())
                .or_else(|| Some(SystemTime::now().into())),
            snapshots,
            poses3d: vec![],
        }))
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
                let do_extrinsic = calibrate_command.do_extrinsic;
                let control_command =
                    camera_control_command::Command::Calibrate(calibrate_command);
                let execution_futures = self
                    .execute_command(which_camera.clone(), control_command)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
                let mut execution_results = join_all(execution_futures).await;
                let num_cameras = execution_results.len();

                // Phase 1: read first message from each camera (intrinsics done)
                let mut calibration_states = Vec::new();
                for (camera, stream) in execution_results.iter_mut() {
                    let maybe_response = stream.next().await;
                    if let Ok(calibration_state) =
                        construct_calibration_state(camera.clone(), maybe_response)
                    {
                        calibration_states.push(calibration_state);
                    }
                }

                // Phase 2: if extrinsic requested, trigger sync TakeSnapshot then read second message
                if do_extrinsic && num_cameras > 0 {
                    let take_snapshot_command =
                        camera_control_command::Command::TakeSnapshot(TakeSnapshotCommand {
                            params: Some(SnapshotParameters {
                                common: Some(SnapshotPayloadParameters {
                                    with_pose: false,
                                    with_image: true,
                                }),
                            }),
                        });
                    let _ = self
                        .execute_command(which_camera.clone(), take_snapshot_command)
                        .await;

                    calibration_states.clear();
                    for (camera, stream) in execution_results.iter_mut() {
                        let maybe_response = stream.next().await;
                        if let Ok(calibration_state) =
                            construct_calibration_state(camera.clone(), maybe_response)
                        {
                            calibration_states.push(calibration_state);
                        }
                    }

                    // Only merge into camera_info if all cameras participated successfully
                    if calibration_states.len() == num_cameras {
                        let mut camera_info_guard = self.camera_info.write();
                        for state in &calibration_states {
                            if let (Some(id), Some(cal)) =
                                (state.which_camera.as_ref(), state.calibration.as_ref())
                            {
                                camera_info_guard.insert(
                                    id.clone(),
                                    CameraInfo {
                                        which_camera: Some(id.clone()),
                                        calibration: Some(cal.clone()),
                                    },
                                );
                            }
                        }
                    }
                } else if !calibration_states.is_empty() {
                    // Intrinsics-only: merge first (and only) response into camera_info
                    let mut camera_info_guard = self.camera_info.write();
                    for state in &calibration_states {
                        if let (Some(id), Some(cal)) =
                            (state.which_camera.as_ref(), state.calibration.as_ref())
                        {
                            camera_info_guard.insert(
                                id.clone(),
                                CameraInfo {
                                    which_camera: Some(id.clone()),
                                    calibration: Some(cal.clone()),
                                },
                            );
                        }
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
            let send_time = Arc::new(Instant::now());

            let response_stream_futures = self
                .execute_command(which_camera, command)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let num_pings = response_stream_futures.len();
            let (ping_tx, ping_rx) = mpsc::channel(num_pings);
            let mapped_futures: Vec<Pin<Box<dyn Future<Output = ()> + Send>>> =
                response_stream_futures
                    .into_iter()
                    .map(|future| {
                        let ping_tx = ping_tx.clone();
                        let send_time = Arc::clone(&send_time);
                        let fut = future.then(move |(_camera, _)| {
                            let ping_tx = ping_tx.clone();
                            let send_time = Arc::clone(&send_time);
                            async move {
                                // Ignore the contents of the message stream - ping only sends a single message.
                                // Once we receive the stream, then the ping has returned.
                                let receive_time = Instant::now();
                                let elapsed = receive_time.duration_since(*send_time);
                                let ping_results = PingResults {
                                    response_time: Some(elapsed.into()),
                                    which_camera: None,
                                };
                                let _ = ping_tx.send(ping_results).await;
                            }
                        });
                        Box::pin(fut) as Pin<Box<dyn Future<Output = ()> + Send>>
                    })
                    .collect();

            // TODO: Set default timeout somewhere else?
            let timeout = maybe_timeout
                .and_then(|t| t.try_into().ok())
                .unwrap_or(Duration::from_secs(5));

            let mut results_buf = Vec::new();
            let watcher = ChannelWatcher::new(ping_rx, &mut results_buf, num_pings);

            // Run response futures (send to ping_tx) and watcher (fill results_buf) in parallel;
            // return when all responses received or timeout.
            let results = select! {
                _ = futures::future::join(
                    futures::future::join_all(mapped_futures),
                    watcher,
                ) => results_buf,
                _ = tokio::time::sleep(timeout) => results_buf
            };

            Ok(Response::new(PingResponse { results }))
        } else {
            // Don't ping any cameras, just respond immediately
            return Ok(Response::new(PingResponse::default()));
        }
    }

    async fn update_cameras(
        &self,
        request: Request<UpdateCamerasRequest>,
    ) -> Result<Response<UpdateCamerasResponse>, Status> {
        let which_camera = request
            .into_inner()
            .which_camera
            .ok_or_else(|| Status::invalid_argument("UpdateCameras requires which_camera"))?;

        let sessions = self.get_sessions(which_camera.clone()).await;
        log::info!(
            "UpdateCameras: which_camera group={:?} camera={:?} -> {} session(s)",
            which_camera.group_name,
            which_camera.camera_name,
            sessions.len()
        );

        let session_tokens: Vec<_> = sessions.iter().map(|s| s.token.clone()).collect();
        let control_channels = self.get_control_channels(&session_tokens).await;

        if control_channels.len() < session_tokens.len() {
            log::warn!(
                "UpdateCameras: {} session(s) but only {} control channel(s) (cameras may not have called CameraControl yet)",
                session_tokens.len(),
                control_channels.len()
            );
        }

        let cameras_updated = control_channels.len() as i32;

        let command_tokens: Vec<_> = control_channels
            .iter()
            .map(|_| CommandToken::new())
            .collect();

        let command = camera_control_command::Command::Update(UpdateCommand {});
        send_control_command(command, control_channels, command_tokens)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        log::info!("UpdateCameras: sent Update command to {} camera(s)", cameras_updated);

        Ok(Response::new(UpdateCamerasResponse { cameras_updated }))
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
            .collect::<Result<Vec<Pose3D>, HubError>>()
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
            let (spoints, _): (Vec<SPoint3>, f64) = initial_pose
                .try_into()
                .map_err(|e: MissingField| Status::invalid_argument(e.to_string()))?;
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

            let camera = view
                .camera
                .ok_or_else(|| Status::invalid_argument("Each view must have camera data"))?;
            let calibration = camera
                .calibration
                .as_ref()
                .ok_or_else(|| Status::invalid_argument("Camera missing calibration"))?;
            let intrinsics = calibration
                .intrinsics
                .as_ref()
                .ok_or_else(|| Status::invalid_argument("Calibration missing intrinsics"))?;
            let extrinsics = calibration
                .extrinsics
                .as_ref()
                .ok_or_else(|| Status::invalid_argument("Calibration missing extrinsics"))?;
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

        let result =
            ceres_bundle_adjustment(&xs, &mut ks, &mut ts, &mut rs, &mut x3d, message.options)
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

#[tonic::async_trait]
impl HubService for Arc<HubServer> {
    type CameraControlStream = <HubServer as HubService>::CameraControlStream;

    async fn hello(&self, request: Request<CameraInfo>) -> Result<Response<SessionToken>, Status> {
        self.as_ref().hello(request).await
    }

    async fn camera_control(
        &self,
        request: Request<SessionToken>,
    ) -> Result<Response<Self::CameraControlStream>, Status> {
        self.as_ref().camera_control(request).await
    }

    async fn camera_data_sink(
        &self,
        request: Request<tonic::Streaming<CameraMessage>>,
    ) -> Result<Response<SendDataSuccess>, Status> {
        self.as_ref().camera_data_sink(request).await
    }

    async fn list_groups(
        &self,
        request: Request<ListGroupsRequest>,
    ) -> Result<Response<ListGroupsResponse>, Status> {
        self.as_ref().list_groups(request).await
    }

    async fn list_cameras(
        &self,
        request: Request<ListCamerasRequest>,
    ) -> Result<Response<ListCamerasResponse>, Status> {
        self.as_ref().list_cameras(request).await
    }

    async fn get_camera_info(
        &self,
        request: Request<CameraIdentifier>,
    ) -> Result<Response<CameraInfo>, Status> {
        self.as_ref().get_camera_info(request).await
    }

    async fn stream_control(
        &self,
        request: Request<StreamControlRequest>,
    ) -> Result<Response<StreamStatus>, Status> {
        self.as_ref().stream_control(request).await
    }

    async fn take_snapshots(
        &self,
        request: Request<ServerSnapshotRequest>,
    ) -> Result<Response<ServerSnapshotResponse>, Status> {
        self.as_ref().take_snapshots(request).await
    }

    async fn get_current(
        &self,
        request: Request<CameraIdentifier>,
    ) -> Result<Response<ServerSnapshotResponse>, Status> {
        self.as_ref().get_current(request).await
    }

    async fn calibrate(
        &self,
        request: Request<CalibrationRequest>,
    ) -> Result<Response<CalibrationResponse>, Status> {
        self.as_ref().calibrate(request).await
    }

    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        self.as_ref().ping(request).await
    }

    async fn update_cameras(
        &self,
        request: Request<UpdateCamerasRequest>,
    ) -> Result<Response<UpdateCamerasResponse>, Status> {
        self.as_ref().update_cameras(request).await
    }

    async fn triangulate(
        &self,
        request: Request<tonic::Streaming<TriangulationRequest>>,
    ) -> Result<Response<TriangulationResponse>, Status> {
        self.as_ref().triangulate(request).await
    }

    async fn bundle_adjustment(
        &self,
        request: Request<BundleAdjustmentRequest>,
    ) -> Result<Response<BundleAdjustmentResponse>, Status> {
        self.as_ref().bundle_adjustment(request).await
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
    rx: mpsc::UnboundedReceiver<CommandResponseMessage>,
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

                let tokens_hm = self.session_tokens.read();
                for (group_name, group_hm_lock) in tokens_hm.iter() {
                    let group_hm = group_hm_lock.read();
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
                let tokens_hm = self.session_tokens.read();
                if let Some(group_hm_lock) = tokens_hm.get(&group_name) {
                    let group_hm = group_hm_lock.read();
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
                let tokens_hm = self.session_tokens.read();
                if let Some(group_hm_lock) = tokens_hm.get(&group_name) {
                    let group_hm = group_hm_lock.read();
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
        let channels_hm = self.control_channels.read();
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
    /// Unbounded so camera_data_sink never blocks when the stream_cache consumer is slow.
    async fn create_data_channels(
        &self,
        tokens: Vec<CommandToken>,
    ) -> Vec<mpsc::UnboundedReceiver<CommandResponseMessage>> {
        let mut rxs = Vec::with_capacity(tokens.len());
        let mut rx_hm = self.data_channels.write();
        for token in tokens {
            let (tx, rx) = mpsc::unbounded_channel();
            rxs.push(rx);
            rx_hm.insert(token, tx);
        }
        rxs
    }

    #[allow(dead_code)]
    async fn retrieve_command_responses(&self) {
        // TODO: What is this supposed to do? Probably useless.
        todo!()
    }

    /// Send command & retrieve results
    async fn execute_command(
        &self,
        which_camera: CameraIdentifier,
        command: camera_control_command::Command,
    ) -> Result<
        Vec<
            impl Future<
                Output = (
                    CameraUniqueIdentifier,
                    impl Stream<Item = CommandResponse> + Send,
                ),
            >,
        >,
        HubError,
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
                let stream_fut = UnboundedReceiverStream::new(rx).into_future();

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
                    // Box::pin so the stream is Unpin and .next().await works in the calibrate handler.
                    let inner_stream = Box::pin(tail_box.filter_map(get_inner_command_response));
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
#[allow(dead_code)]
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
    hub: Arc<HubServer>,
}

impl GrpcServer {
    pub fn new(config: GrpcConfig, hub: Arc<HubServer>) -> Self {
        Self { config, hub }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        log::info!("PoseNet Hub gRPC service listening on {}", self.config.addr);

        Server::builder()
            .add_service(HubServiceServer::new(self.hub))
            .serve(self.config.addr)
            .await?;

        Ok(())
    }
}

// TODO: Move these tests elsewhere?
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc;
    use tokio::time::{sleep, timeout};
    use tonic::transport::Channel;
    use tonic::Request;

    use crate::grpc::client as grpc_client;
    use crate::grpc::proto::hub_service_client::HubServiceClient;
    use crate::grpc::proto::CameraIdentifier;
    use crate::grpc::proto::CameraInfo;
    use crate::grpc::proto::CameraExtrinsics;
    use crate::grpc::proto::CameraIntrinsics;
    use crate::grpc::proto::CalibrationParameters;
    use crate::grpc::proto::PingRequest;
    use crate::grpc::proto::SessionToken;
    use crate::grpc::proto::{BundleAdjustmentOptions, BundleAdjustmentRequest, Point2D, Point3D, Pose2D, Pose3D, TriangulationRequest};

    const TEST_PORT_BASE: u16 = 50200;
    const SERVER_STARTUP_MS: u64 = 200;
    const TEST_TIMEOUT: Duration = Duration::from_secs(15);
    static NEXT_TEST_PORT: AtomicU16 = AtomicU16::new(TEST_PORT_BASE);

    async fn start_test_server() -> u16 {
        let port = NEXT_TEST_PORT.fetch_add(1, Ordering::SeqCst);
        let (cameras_tx, cameras_rx) = mpsc::unbounded_channel();
        let (snapshots_tx, snapshots_rx) = mpsc::unbounded_channel();
        // Keep receivers alive so Hello can send camera info.
        tokio::spawn(async move {
            let _ = cameras_rx;
            let _ = snapshots_rx;
            std::future::pending::<()>().await
        });
        let hub = Arc::new(crate::grpc::server::HubServer::new(cameras_tx, snapshots_tx, None));
        let config = crate::grpc::server::GrpcConfig::new("127.0.0.1", port).expect("GrpcConfig");
        let grpc_server = crate::grpc::server::GrpcServer::new(config, hub);
        tokio::spawn(async move {
            let _ = grpc_server.run().await;
        });
        sleep(Duration::from_millis(SERVER_STARTUP_MS)).await;
        port
    }

    async fn connect_client(port: u16) -> HubServiceClient<Channel> {
        let addr = format!("http://127.0.0.1:{}", port);
        timeout(TEST_TIMEOUT, HubServiceClient::connect(addr))
            .await
            .expect("connect timeout")
            .expect("client connect")
    }

    fn camera_info_for_triangulation(group_name: &str, camera_name: &str, view_matrix: Vec<f64>) -> CameraInfo {
        CameraInfo {
            which_camera: Some(CameraIdentifier {
                group_name: group_name.to_string(),
                camera_name: camera_name.to_string(),
            }),
            calibration: Some(CalibrationParameters {
                intrinsics: Some(CameraIntrinsics {
                    camera_matrix: vec![500.0, 0.0, 320.0, 0.0, 500.0, 240.0, 0.0, 0.0, 1.0],
                    distortion: vec![0.0, 0.0, 0.0, 0.0, 0.0],
                    rms_error: 0.0,
                }),
                extrinsics: Some(CameraExtrinsics { view_matrix }),
            }),
        }
    }

    fn full_pose2d(x: f64, y: f64, score: f64) -> Pose2D {
        let pt = Point2D { x, y, score };
        Pose2D {
            nose: Some(pt.clone()),
            left_eye: Some(pt.clone()),
            right_eye: Some(pt.clone()),
            left_ear: Some(pt.clone()),
            right_ear: Some(pt.clone()),
            left_shoulder: Some(pt.clone()),
            right_shoulder: Some(pt.clone()),
            left_elbow: Some(pt.clone()),
            right_elbow: Some(pt.clone()),
            left_wrist: Some(pt.clone()),
            right_wrist: Some(pt.clone()),
            left_hip: Some(pt.clone()),
            right_hip: Some(pt.clone()),
            left_knee: Some(pt.clone()),
            right_knee: Some(pt.clone()),
            left_ankle: Some(pt.clone()),
            right_ankle: Some(pt),
            score,
        }
    }

    fn full_pose3d(z: f64, score: f64) -> Pose3D {
        let pt = Point3D { x: 0.0, y: 0.0, z, score: 1.0 };
        Pose3D {
            nose: Some(pt.clone()),
            left_eye: Some(pt.clone()),
            right_eye: Some(pt.clone()),
            left_ear: Some(pt.clone()),
            right_ear: Some(pt.clone()),
            left_shoulder: Some(pt.clone()),
            right_shoulder: Some(pt.clone()),
            left_elbow: Some(pt.clone()),
            right_elbow: Some(pt.clone()),
            left_wrist: Some(pt.clone()),
            right_wrist: Some(pt.clone()),
            left_hip: Some(pt.clone()),
            right_hip: Some(pt.clone()),
            left_knee: Some(pt.clone()),
            right_knee: Some(pt.clone()),
            left_ankle: Some(pt.clone()),
            right_ankle: Some(pt),
            score,
        }
    }

    #[tokio::test]
    async fn test_hello() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let name = grpc_client::hello(&mut client, "unit_hello_group".to_string())
                .await
                .expect("hello");
            assert!(!name.is_empty());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_camera_control() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;

            // CameraControl requires a valid SessionToken returned by Hello.
            // Using an unknown token should fail with FailedPrecondition.
            let bad = SessionToken { data: "not-a-real-token".to_string() };
            let res = client.camera_control(Request::new(bad)).await;
            assert!(res.is_err());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_camera_data_sink() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;

            // CameraDataSink requires the stream to begin with a CommandToken.
            // An empty stream should fail with InvalidArgument.
            let (tx, rx) = tokio::sync::mpsc::channel::<crate::grpc::proto::CameraMessage>(1);
            drop(tx);
            let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
            let res = client.camera_data_sink(Request::new(request_stream)).await;
            assert!(res.is_err());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_list_groups() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let groups = grpc_client::list_groups(&mut client).await.expect("list_groups");
            assert!(groups.is_empty());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_list_cameras() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let group = "unit_list_cameras_group".to_string();
            let camera_name = grpc_client::hello(&mut client, group.clone())
                .await
                .expect("hello");
            let cameras = grpc_client::list_cameras(&mut client, group).await.expect("list_cameras");
            assert_eq!(cameras, vec![camera_name]);
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_get_camera_info() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let group = "unit_get_camera_info_group".to_string();
            let camera_name = grpc_client::hello(&mut client, group.clone())
                .await
                .expect("hello");
            let which = CameraIdentifier {
                group_name: group,
                camera_name,
            };
            let info = grpc_client::get_camera_info(&mut client, which)
                .await
                .expect("get_camera_info");
            assert!(info.which_camera.is_some());
            assert!(info.calibration.is_some());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_stream_control() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let group = "unit_stream_control_group".to_string();
            let status = grpc_client::stream_control_start(&mut client, group.clone(), true, false, None)
                .await
                .expect("stream_control_start");
            assert!(status.is_streaming);
            let status = grpc_client::stream_control_stop(&mut client, group)
                .await
                .expect("stream_control_stop");
            assert!(!status.is_streaming);
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_take_snapshots() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let resp = grpc_client::get_snapshots(&mut client, "unit_snap_group".to_string(), true, false, false)
                .await
                .expect("get_snapshots");
            assert!(!resp.snapshot_id.is_empty());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_get_current() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            // Without a prior snapshot, GetCurrent should error.
            let which = CameraIdentifier {
                group_name: "unit_current_group".to_string(),
                camera_name: String::new(),
            };
            assert!(grpc_client::get_current(&mut client, which).await.is_err());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_calibrate() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let req = grpc_client::build_calibration_request(
                "unit_cal_group".to_string(),
                None,
                false,
                true,
                None,
                None,
                None,
            );
            let resp = grpc_client::calibrate(&mut client, req).await.expect("calibrate");
            // No cameras connected in this group -> empty.
            assert!(resp.states.is_empty());
        })
        .await
        .expect("test timeout");
    }

    #[test]
    fn test_construct_calibration_state_success() {
        use crate::grpc::proto::command_response;
        use crate::grpc::proto::{CalibrationParameters, CommandResponse};
        use crate::grpc::proto::{CameraExtrinsics, CameraIntrinsics, CameraUniqueIdentifier};
        use super::construct_calibration_state;

        let camera = CameraUniqueIdentifier {
            group_name: "g".to_string(),
            camera_name: "c".to_string(),
        };
        let params = CalibrationParameters {
            intrinsics: Some(CameraIntrinsics {
                camera_matrix: vec![1.0; 9],
                distortion: vec![0.0; 5],
                rms_error: 0.0,
            }),
            extrinsics: Some(CameraExtrinsics {
                view_matrix: vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            }),
        };
        let response = CommandResponse {
            response: Some(command_response::Response::Calibration(params.clone())),
        };
        let out = construct_calibration_state(camera, Some(response)).expect("ok");
        assert_eq!(out.which_camera.as_ref().unwrap().group_name, "g");
        assert_eq!(out.which_camera.as_ref().unwrap().camera_name, "c");
        assert!(out.calibration.is_some());
        assert_eq!(out.calibration.as_ref().unwrap().intrinsics.as_ref().unwrap().camera_matrix.len(), 9);
    }

    #[test]
    fn test_construct_calibration_state_none_response() {
        use crate::grpc::proto::CameraUniqueIdentifier;
        use super::construct_calibration_state;

        let camera = CameraUniqueIdentifier {
            group_name: "g".to_string(),
            camera_name: "c".to_string(),
        };
        let out = construct_calibration_state(camera, None);
        assert!(out.is_err());
    }

    #[test]
    fn test_construct_calibration_state_wrong_response_type() {
        use crate::grpc::proto::command_response;
        use crate::grpc::proto::{CommandResponse, Snapshot};
        use crate::grpc::proto::CameraUniqueIdentifier;
        use super::construct_calibration_state;

        let camera = CameraUniqueIdentifier {
            group_name: "g".to_string(),
            camera_name: "c".to_string(),
        };
        let response = CommandResponse {
            response: Some(command_response::Response::Snapshot(Snapshot::default())),
        };
        let out = construct_calibration_state(camera, Some(response));
        assert!(out.is_err());
    }

    #[tokio::test]
    async fn test_ping() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let resp = grpc_client::ping(
                &mut client,
                PingRequest {
                    which_camera: None,
                    timeout: None,
                },
            )
            .await
            .expect("ping");
            assert!(resp.results.is_empty());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_update_cameras() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;
            let req = grpc_client::build_update_cameras_request(CameraIdentifier {
                group_name: "unit_update_group".to_string(),
                camera_name: String::new(),
            });
            let resp = grpc_client::update_cameras(&mut client, req)
                .await
                .expect("update_cameras");
            assert!(resp.cameras_updated >= 0);
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_triangulate() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;

            // Minimal triangulate smoke test: empty stream should succeed with empty response.
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            drop(tx);
            let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
            let resp = client
                .triangulate(Request::new(stream))
                .await
                .expect("triangulate")
                .into_inner();
            assert!(resp.poses.is_empty());
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn test_bundle_adjustment() {
        timeout(TEST_TIMEOUT, async {
            let port = start_test_server().await;
            let mut client = connect_client(port).await;

            // Valid bundle adjustment request (similar to integration test) to avoid triggering
            // assertions in the C++/Eigen layer for empty inputs.
            let cam1 = camera_info_for_triangulation(
                "ba_unit",
                "c1",
                vec![
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ],
            );
            let cam2 = camera_info_for_triangulation(
                "ba_unit",
                "c2",
                vec![
                    1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ],
            );
            let pose1 = full_pose2d(320.0, 240.0, 1.0);
            let pose2 = full_pose2d(330.0, 240.0, 1.0);
            let initial_pose = full_pose3d(5.0, 1.0);

            let req = BundleAdjustmentRequest {
                views: vec![
                    TriangulationRequest {
                        camera: Some(cam1),
                        poses: vec![pose1],
                    },
                    TriangulationRequest {
                        camera: Some(cam2),
                        poses: vec![pose2],
                    },
                ],
                initial_poses: vec![initial_pose],
                options: Some(BundleAdjustmentOptions {
                    camera_rotation: true,
                    camera_translation: true,
                    camera_intrinsics: false,
                    pose3d: true,
                }),
            };

            let response = client
                .bundle_adjustment(Request::new(req))
                .await
                .expect("bundle_adjustment")
                .into_inner();
            assert_eq!(response.cameras.len(), 2);
            assert_eq!(response.poses.len(), 1);
        })
        .await
        .expect("test timeout");
    }
}
