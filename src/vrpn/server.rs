use async_std::channel;
use std::error::Error;

use crate::grpc::proto::Pose3D;
use crate::utils::read_rx;

pub async fn main(poses3d_rx: channel::Receiver<Pose3D>) -> Result<(), Box<dyn Error>> {
    read_rx("Pose 3D", poses3d_rx).await?;

    Ok(())
}
