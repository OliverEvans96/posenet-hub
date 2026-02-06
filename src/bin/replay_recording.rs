//! Replay a recorded .pnhr file to the hub as if cameras were streaming.
//!
//! Registers one virtual camera per (group, camera) in the file, then when the hub
//! sends StartStreaming, streams snapshots from the file in timestamp order.

use futures::StreamExt;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use structopt::StructOpt;
use tokio::sync::mpsc;
use tokio::time::interval;
use tonic::transport::Channel;
use tonic::Request;

use posenet_vr_hub::grpc::proto::hub_service_client::HubServiceClient;
use posenet_vr_hub::grpc::proto::{
    camera_control_command, camera_message, command_response, CalibrationParameters,
    CameraIdentifier, CameraInfo, CameraMessage, CommandResponse, CommandToken, SessionToken,
    Snapshot,
};
use posenet_vr_hub::recording::read_recording;

#[derive(Debug, StructOpt)]
#[structopt(name = "replay_recording", about = "Replay a .pnhr recording file to the hub")]
struct Args {
    /// Path to the .pnhr recording file
    #[structopt(short, long, parse(from_os_str))]
    file: PathBuf,

    /// Hub gRPC URL (e.g. http://127.0.0.1:50051)
    #[structopt(short, long, default_value = "http://127.0.0.1:50051")]
    hub_url: String,

    /// Replay in real time (sleep according to timestamps); default is as-fast-as-possible
    #[structopt(long)]
    realtime: bool,
}

fn dummy_calibration() -> CalibrationParameters {
    use posenet_vr_hub::grpc::proto::{CameraExtrinsics, CameraIntrinsics};
    CalibrationParameters {
        intrinsics: Some(CameraIntrinsics {
            camera_matrix: vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            distortion: vec![0.0; 5],
            rms_error: 0.0,
        }),
        extrinsics: Some(CameraExtrinsics {
            view_matrix: vec![
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }),
    }
}

async fn run_camera_control_listener(
    mut client: HubServiceClient<Channel>,
    session_token: SessionToken,
    camera_name: String,
    group_name: String,
    mut snapshot_rx: mpsc::UnboundedReceiver<Snapshot>,
    realtime: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let which_camera = CameraIdentifier {
        group_name: group_name.clone(),
        camera_name: camera_name.clone(),
    };
    let request = Request::new(session_token);
    let mut stream = client.camera_control(request).await?.into_inner();
    while let Some(msg) = stream.next().await {
        let cmd = msg?;
        match (&cmd.token, &cmd.command) {
            (Some(token), Some(camera_control_command::Command::StartStreaming(_))) => {
                let command_token = token.clone();
                let mut data_sink_client = client.clone();
                let which_cam = which_camera.clone();
                let rx = std::mem::replace(&mut snapshot_rx, mpsc::unbounded_channel().1);
                tokio::spawn(async move {
                    if let Err(e) = run_camera_data_sink(
                        &mut data_sink_client,
                        command_token,
                        which_cam,
                        rx,
                        realtime,
                    )
                    .await
                    {
                        log::error!("CameraDataSink error: {}", e);
                    }
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn timestamp_to_nanos(snap: &Snapshot) -> i64 {
    snap.timestamp
        .as_ref()
        .map(|t| t.seconds * 1_000_000_000 + t.nanos as i64)
        .unwrap_or(0)
}

async fn run_camera_data_sink(
    client: &mut HubServiceClient<Channel>,
    command_token: CommandToken,
    _which_camera: CameraIdentifier,
    mut snapshot_rx: mpsc::UnboundedReceiver<Snapshot>,
    realtime: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token_msg = CameraMessage {
        msg: Some(camera_message::Msg::Token(command_token)),
    };
    let stream = async_stream::stream! {
        yield token_msg;
        let mut prev_ts_nanos: Option<i64> = None;
        while let Some(snap) = snapshot_rx.recv().await {
            if realtime {
                let ts = timestamp_to_nanos(&snap);
                if let Some(prev) = prev_ts_nanos {
                    let delta_ns = (ts - prev).max(0);
                    tokio::time::sleep(Duration::from_nanos(delta_ns as u64)).await;
                }
                prev_ts_nanos = Some(ts);
            }
            yield CameraMessage {
                msg: Some(camera_message::Msg::Response(CommandResponse {
                    response: Some(command_response::Response::Snapshot(snap)),
                })),
            };
        }
    };
    let request = Request::new(stream);
    let _response = client.camera_data_sink(request).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let args = Args::from_args();
    let snapshots = read_recording(&args.file)?;
    if snapshots.is_empty() {
        anyhow::bail!("Recording file has no snapshots");
    }

    type CameraKey = (String, String);
    let mut by_camera: HashMap<CameraKey, Vec<Snapshot>> = HashMap::new();
    for snap in snapshots {
        let key = snap
            .which_camera
            .as_ref()
            .map(|id| (id.group_name.clone(), id.camera_name.clone()))
            .unwrap_or_else(|| (String::new(), String::new()));
        if !key.0.is_empty() && !key.1.is_empty() {
            by_camera.entry(key).or_default().push(snap);
        }
    }
    for v in by_camera.values_mut() {
        v.sort_by_key(timestamp_to_nanos);
    }

    let hub_url = args.hub_url.clone();
    let realtime = args.realtime;
    let mut cameras: Vec<(CameraKey, Vec<Snapshot>)> = by_camera.into_iter().collect();
    cameras.sort_by(|a, b| a.0.cmp(&b.0));

    for ((group_name, camera_name), snaps) in &cameras {
        log::info!(
            "Camera {}:{} has {} snapshots",
            group_name,
            camera_name,
            snaps.len()
        );
    }

    let _channel = Channel::from_shared(hub_url.clone())?.connect().await?;

    let mut join_handles = Vec::new();
    for ((group_name, camera_name), snapshots_for_cam) in cameras {
        let (tx, rx) = mpsc::unbounded_channel();
        for s in snapshots_for_cam {
            let _ = tx.send(s);
        }
        drop(tx);

        let hub_url = hub_url.clone();
        let group_name = group_name.clone();
        let camera_name = camera_name.clone();
        join_handles.push(tokio::spawn(async move {
            let channel = Channel::from_shared(hub_url).unwrap().connect().await.unwrap();
            let mut client = HubServiceClient::new(channel);
            let which_camera = CameraIdentifier {
                group_name: group_name.clone(),
                camera_name: camera_name.clone(),
            };
            let info = CameraInfo {
                which_camera: Some(which_camera),
                calibration: Some(dummy_calibration()),
            };
            let request = Request::new(info);
            let session_token = match client.hello(request).await {
                Ok(r) => r.into_inner(),
                Err(e) => {
                    log::error!("Hello failed for {}:{}: {}", group_name, camera_name, e);
                    return;
                }
            };
            log::info!("Registered {}:{}", group_name, camera_name);
            if let Err(e) = run_camera_control_listener(
                client,
                session_token,
                camera_name,
                group_name,
                rx,
                realtime,
            )
            .await
            {
                log::error!("CameraControl listener error: {}", e);
            }
        }));
    }

    for h in join_handles {
        let _ = h.await;
    }
    Ok(())
}
