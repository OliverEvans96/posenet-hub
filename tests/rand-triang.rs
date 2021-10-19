// use futures::future;
// use nalgebra::Matrix3x4;
// use rand::Rng;
// use std::thread;
// use std::time::Duration;

// use posenet_vr_hub::grpc::proto::Pose3D;
// use posenet_vr_hub::triangulator::triangulate_from_poses_and_camera_matrices;

// fn randomly_triangulate() -> Pose3D {
//     let mut rng = rand::thread_rng();
//     let num_cameras = 5;
//     let poses = (0..num_cameras).map(|_| rng.gen()).collect();
//     let camera_matrices = (0..num_cameras).map(|_| Matrix3x4::new_random()).collect();
//     return triangulate_from_poses_and_camera_matrices(poses, camera_matrices);
// }

// #[test]
// fn single_thread() {
//     for _ in 0..10 {
//         for _ in 0..3 {
//             randomly_triangulate();
//             thread::sleep(Duration::from_millis(20));
//             randomly_triangulate();
//             thread::sleep(Duration::from_millis(20));
//         }
//     }
// }

// #[test]
// fn one_more_thread() {
//     let handle = thread::spawn(|| {
//         randomly_triangulate();
//         thread::sleep(Duration::from_millis(20));
//         randomly_triangulate();
//         thread::sleep(Duration::from_millis(20));
//     });
//     handle.join().expect("failed to join");
// }

// #[test]
// fn multi_thread() {
//     let handles = (0..10).map(|_| {
//         thread::spawn(|| {
//             randomly_triangulate();
//             thread::sleep(Duration::from_millis(20));
//             randomly_triangulate();
//             thread::sleep(Duration::from_millis(20));
//         })
//     });
//     handles.for_each(|h| h.join().expect("failed to join"));
// }

// #[tokio::test]
// async fn multi_task() {
//     let futures = (0u8..10).map(|_| {
//         tokio::spawn(async {
//             randomly_triangulate();
//             tokio::time::sleep(Duration::from_millis(20)).await;
//             randomly_triangulate();
//             tokio::time::sleep(Duration::from_millis(20)).await;
//         })
//     });
//     let result = future::try_join_all(futures).await;
//     assert!(result.is_ok());
// }
