// use async_std::channel;
use std::net::SocketAddr;
use tokio::sync::broadcast;

use super::vrpn::{ffi, update_values};
use crate::triangulator::LabeledPoses3D;

#[derive(Clone, Debug)]
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
}

impl VrpnServer {
    pub fn new(config: VrpnConfig, poses3d_rx: broadcast::Receiver<LabeledPoses3D>) -> Self {
        Self { config, poses3d_rx }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        log::info!("PoseNet Hub VRPN service listening on {}", self.config.addr);

        let mut container = ffi::create_container();
        let mut seen_groups: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut update_count: u64 = 0;

        loop {
            let message = self.poses3d_rx.recv().await?;
            let device_name = message.group_name.clone() + ":" + &self.config.device_name;
            let num_poses = message.poses.len();

            if seen_groups.insert(message.group_name.clone()) {
                log::info!(
                    "VRPN: new group '{}' → device '{}' ({} pose(s) in this update)",
                    message.group_name,
                    device_name,
                    num_poses
                );
            }

            if let Some(pose) = message.poses.into_iter().next() {
                let score = pose.score;
                update_values(&mut container, &device_name, pose)?;
                update_count = update_count.saturating_add(1);
                log::trace!(
                    "VRPN update #{} for '{}' (pose score: {:.3})",
                    update_count,
                    device_name,
                    score
                );
            } else if num_poses > 0 {
                log::warn!(
                    "VRPN: group '{}' had {} pose(s) but none could be used",
                    message.group_name,
                    num_poses
                );
            }

            ffi::mainloop(&mut container);
        }
    }
}
