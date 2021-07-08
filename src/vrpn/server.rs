// use async_std::channel;
use tokio::{sync::broadcast};
use std::{error::Error, net::SocketAddr};

use super::vrpn::{ffi, update_values};
use crate::triangulator::LabeledPoses3D;

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
        VrpnConfig::new("PoseNet0", "0.0.0.0", 3883).expect("Default VRPN configuration invalid!")
    }
}

pub struct VrpnServer {
    config: VrpnConfig,
    poses3d_rx: broadcast::Receiver<LabeledPoses3D>,
    // poses3d_rx: channel::Receiver<Option<Pose3D>>,
}

impl VrpnServer {
    pub fn new(
        config: VrpnConfig,
        poses3d_rx: broadcast::Receiver<LabeledPoses3D>
    ) -> Self {
        Self { 
            config,
            poses3d_rx
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("PoseNet Hub VRPN service listening on {}", self.config.addr);

        // TODO use config addr in create_server..
        // let mut server = ffi::create_server(&self.config.device_name); 
        let mut container = ffi::create_container();
        loop {
            // Check for new pose from controller
            let message = self.poses3d_rx.recv().await?;

            // Update values from pose if available
            if message.poses.len() > 0 {
                let device_name = message.group_name + ".pose0";
                update_values(&mut container, &device_name, message.poses.into_iter().nth(0).unwrap());
                // update_values(&mut server, message.poses.first().unwrap());
            }

            // Talk to clients
            ffi::mainloop(&mut container);
        }
    }
}
