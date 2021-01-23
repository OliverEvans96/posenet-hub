use async_std::channel;
use std::{error::Error, net::SocketAddr};

use super::vrpn::{ffi, mainloop};
use crate::grpc::proto::Pose3D;

pub struct VrpnConfig {
    device_name: String,
    addr: SocketAddr,
}

impl VrpnConfig {
    pub fn new(device_name: &str, ip: &str, port: u16) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            device_name: device_name.to_owned(),
            addr: format!("{}:{}", ip, port).parse()?,
        })
    }
}

impl Default for VrpnConfig {
    fn default() -> Self {
        VrpnConfig::new("PoseNet0", "[::1]", 3038).expect("Default VRPN configuration invalid!")
    }
}

pub struct VrpnServer {
    config: VrpnConfig,
    poses3d_rx: channel::Receiver<Pose3D>,
}

impl VrpnServer {
    pub fn new(config: VrpnConfig, poses3d_rx: channel::Receiver<Pose3D>) -> Self {
        Self { config, poses3d_rx }
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        println!("PoseNet Hub VRPN service listening on {}", self.config.addr);

        let mut server = ffi::create_server(&self.config.device_name);
        loop {
            let pose = self.poses3d_rx.recv().await?;
            mainloop(&mut server, pose);
        }
    }
}
