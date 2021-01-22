use nalgebra::{Matrix3x4, Point3};
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

use posenet_vr_hub::controller::triangulate_from_poses_and_camera_matrices;
use posenet_vr_hub::grpc::client::random_pose;
use posenet_vr_hub::grpc::server::LabeledPose2D;

pub fn randomly_triangulate() -> Vec<Point3<f64>> {
    let num_cameras = 5;
    let poses = (0..num_cameras)
        .map(|_| LabeledPose2D {
            name: "random".to_owned(),
            time: Instant::now(),
            pose: random_pose(),
        })
        .collect();
    let camera_matrices = (0..num_cameras).map(|_| Matrix3x4::new_random()).collect();
    return triangulate_from_poses_and_camera_matrices(poses, camera_matrices);
}
fn main() -> ! {
    loop {
        randomly_triangulate();
        sleep(Duration::from_millis(1000));
    }
}
