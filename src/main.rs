use async_std::channel;
use std::error::Error;
use tokio::join;

mod grpc;
mod vrpn;

use grpc::proto::Pose2DMessage;
use grpc::server::NamedCameraInfo;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (cameras_tx, cameras_rx) = channel::unbounded::<NamedCameraInfo>();
    let (poses_tx, poses_rx) = channel::unbounded::<Pose2DMessage>();
    let grpc_fut = grpc::server::main(cameras_tx, poses_tx);
    let vrpn_fut = vrpn::server::main(cameras_rx, poses_rx);

    let (grpc_result, vrpn_result) = join!(grpc_fut, vrpn_fut);

    Ok(())
}
