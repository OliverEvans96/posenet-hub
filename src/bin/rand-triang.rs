use nalgebra::{Matrix3x4, Point3};
use std::thread;
use std::time::{Duration, Instant};

use posenet_vr_hub::controller::triangulate_from_poses_and_camera_matrices;
use posenet_vr_hub::grpc::client::random_pose;
use posenet_vr_hub::grpc::server::LabeledPose2D;

type AsyncResult = Result<(), Box<dyn std::error::Error>>;

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

fn single_thread_okay() {
    loop {
        for _ in 0..3 {
            randomly_triangulate();
            thread::sleep(Duration::from_millis(1000));
        }
    }
}

fn one_more_thread_segfault() {
    let handle = thread::spawn(|| {
        randomly_triangulate();
        thread::sleep(Duration::from_millis(1000));
    });
    handle.join().expect("failed to join");
}

fn multi_thread_segfault() {
    let handles = (0..10).map(|_| {
        thread::spawn(|| {
            randomly_triangulate();
            thread::sleep(Duration::from_millis(1000));
        })
    });
    handles.for_each(|h| h.join().expect("failed to join"));
}

async fn async_segfault() -> AsyncResult {
    async fn inner() -> AsyncResult {
        randomly_triangulate();
        tokio::time::delay_for(Duration::from_millis(1000)).await;
        Ok(())
    }
    loop {
        tokio::try_join!(inner(), inner(), inner()).unwrap();
    }
}

fn main() {
    // single_thread__okay();
    one_more_thread_segfault();
    // multi_thread_segfault();
    // async_segfault().await?;
}
