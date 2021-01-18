use async_std::channel;
use std::{collections::HashMap, error::Error, time::Duration};
use tokio::try_join;

use crate::grpc::proto::{CameraInfo, Pose3D};
use crate::grpc::server::{LabeledPose2D, NamedCameraInfo};

pub enum ControllerAlgorithm {
    LatestPoseTimeLimit(Duration),
}

struct CameraManager {
    cameras_rx: channel::Receiver<NamedCameraInfo>,
    hm: HashMap<String, CameraInfo>,
}

impl CameraManager {
    async fn run(&self) -> Result<(), Box<dyn Error>> {
        // TODO
        Ok(())
    }

    fn new(cameras_rx: channel::Receiver<NamedCameraInfo>) -> Self {
        Self {
            cameras_rx,
            hm: HashMap::<String, CameraInfo>::new(),
        }
    }
}

struct PoseManager {
    poses2d_rx: channel::Receiver<LabeledPose2D>,
    hm: HashMap<String, LabeledPose2D>,
}

impl PoseManager {
    async fn run(&self) -> Result<(), Box<dyn Error>> {
        // TODO
        Ok(())
    }

    fn new(poses2d_rx: channel::Receiver<LabeledPose2D>) -> Self {
        Self {
            poses2d_rx,
            hm: HashMap::<String, LabeledPose2D>::new(),
        }
    }
}

struct TriangulationManager {
    // TODO: How does the TM work?
}

impl TriangulationManager {
    async fn run(&self) -> Result<(), Box<dyn Error>> {
        // TODO
        Ok(())
    }
    fn new() -> Self {
        Self {}
    }
}

impl ControllerAlgorithm {
    pub async fn run(
        &self,
        cameras_rx: channel::Receiver<NamedCameraInfo>,
        poses2d_rx: channel::Receiver<LabeledPose2D>,
        poses3d_tx: channel::Sender<Pose3D>,
    ) -> Result<(), Box<dyn Error>> {
        match self {
            ControllerAlgorithm::LatestPoseTimeLimit(duration) => {
                let cm = CameraManager::new(cameras_rx);
                let pm = PoseManager::new(poses2d_rx);
                let tm = TriangulationManager::new();
                try_join!(cm.run(), pm.run(), tm.run())?;
            }
        }

        Ok(())
    }
}
