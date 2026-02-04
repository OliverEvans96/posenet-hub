// use async_std::channel;
use std::net::SocketAddr;
use tokio::sync::broadcast;

use super::vrpn::{ffi, update_values};
use crate::triangulator::LabeledPoses3D;

pub struct VrpnConfig {
    device_name: String,
    addr: SocketAddr,
}

impl VrpnConfig {
    pub fn new(device_name: &str, ip: &str, port: u16) -> anyhow::Result<Self> {
        Ok(Self {
            device_name: device_name.to_owned(),
            addr: format!("{}:{}", ip, port).parse()?,
        })
    }

    pub fn try_default() -> anyhow::Result<Self> {
        Self::new("PoseNet0", "0.0.0.0", 3883)
    }
}

impl Default for VrpnConfig {
    fn default() -> Self {
        Self::try_default().expect("default VRPN config 0.0.0.0:3883 must be valid")
    }
}

pub struct VrpnServer {
    config: VrpnConfig,
    poses3d_rx: broadcast::Receiver<LabeledPoses3D>,
    // poses3d_rx: channel::Receiver<Option<Pose3D>>,
}

impl VrpnServer {
    pub fn new(config: VrpnConfig, poses3d_rx: broadcast::Receiver<LabeledPoses3D>) -> Self {
        Self { config, poses3d_rx }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        println!("PoseNet Hub VRPN service listening on {}", self.config.addr);

        let mut container = ffi::create_container();
        loop {
            let message = self.poses3d_rx.recv().await?;
            let device_name = message.group_name + ":" + &self.config.device_name;
            println!("VRPN device: {}", device_name);

            if let Some(pose) = message.poses.into_iter().next() {
                update_values(&mut container, &device_name, pose)?;
            }

            ffi::mainloop(&mut container);
        }
    }
}
