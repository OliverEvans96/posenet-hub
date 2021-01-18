use async_std::channel;
use std::error::Error;
use std::fmt::Debug;
use tokio::try_join;

use crate::grpc::proto::Pose2DMessage;
use crate::grpc::server::NamedCameraInfo;

async fn read_rx<T: Debug>(name: &str, rx: channel::Receiver<T>) -> Result<(), Box<dyn Error>> {
    println!("Listening for {}", name);
    let mut i = 0;
    loop {
        let thing = rx.recv().await?;
        i += 1;
        println!("#{} {}: {:#?}", i, name, thing);
    }
}

pub async fn main(
    cameras_rx: channel::Receiver<NamedCameraInfo>,
    poses_rx: channel::Receiver<Pose2DMessage>,
) -> Result<(), Box<dyn Error>> {
    try_join!(read_rx("camera", cameras_rx), read_rx("pose", poses_rx))?;

    Ok(())
}
