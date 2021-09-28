tonic::include_proto!("posenet_vr");

use rand::{distributions::Standard, prelude::Distribution};
use std::convert::TryInto;

use crate::utils::pop_n;

// PoseNet returns 17 points on the body
const NUM_KEYPOINTS: usize = 17;
const NDIM: usize = 4;
// (x,y,z, score) for 17 keypoints + pose score
const NUM_CHANNELS: usize = NDIM * NUM_KEYPOINTS + 1;

// type aliases for scored nalgebra tuples
pub type SPoint2 = (nalgebra::Point2<f64>, f64);
pub type SPoint3 = (nalgebra::Point3<f64>, f64);

// Convert between gRPC Point and nalgebra::Point

impl From<SPoint2> for Point2D {
    fn from(t: SPoint2) -> Self {
        let (p, s) = t;
        Self {
            x: p.x,
            y: p.y,
            score: s,
        }
    }
}

impl From<Point2D> for SPoint2 {
    fn from(p: Point2D) -> Self {
        (nalgebra::Point2::new(p.x, p.y), p.score)
    }
}

impl From<SPoint3> for Point3D {
    fn from(t: SPoint3) -> Self {
        let (p, s) = t;
        Self {
            x: p.x,
            y: p.y,
            z: p.z,
            score: s,
        }
    }
}

impl From<Point3D> for SPoint3 {
    fn from(p: Point3D) -> Self {
        (nalgebra::Point3::new(p.x, p.y, p.z), p.score)
    }
}

// Convert between gRPC Pose and (Vec<nalgebra::Point>, f64)

impl From<(Vec<SPoint2>, f64)> for Pose2D {
    fn from(t: (Vec<SPoint2>, f64)) -> Self {
        // TODO: Allow missing points
        let (v, s) = t;
        Self {
            nose: Some(v[0].into()),
            left_eye: Some(v[1].into()),
            right_eye: Some(v[2].into()),
            left_ear: Some(v[3].into()),
            right_ear: Some(v[4].into()),
            left_shoulder: Some(v[5].into()),
            right_shoulder: Some(v[6].into()),
            left_elbow: Some(v[7].into()),
            right_elbow: Some(v[8].into()),
            left_wrist: Some(v[9].into()),
            right_wrist: Some(v[10].into()),
            left_hip: Some(v[11].into()),
            right_hip: Some(v[12].into()),
            left_knee: Some(v[13].into()),
            right_knee: Some(v[14].into()),
            left_ankle: Some(v[15].into()),
            right_ankle: Some(v[16].into()),
            score: s,
        }
    }
}

impl From<(Vec<SPoint3>, f64)> for Pose3D {
    fn from(t: (Vec<SPoint3>, f64)) -> Self {
        // TODO: Allow missing points
        let (v, s) = t;
        Self {
            nose: Some(v[0].into()),
            left_eye: Some(v[1].into()),
            right_eye: Some(v[2].into()),
            left_ear: Some(v[3].into()),
            right_ear: Some(v[4].into()),
            left_shoulder: Some(v[5].into()),
            right_shoulder: Some(v[6].into()),
            left_elbow: Some(v[7].into()),
            right_elbow: Some(v[8].into()),
            left_wrist: Some(v[9].into()),
            right_wrist: Some(v[10].into()),
            left_hip: Some(v[11].into()),
            right_hip: Some(v[12].into()),
            left_knee: Some(v[13].into()),
            right_knee: Some(v[14].into()),
            left_ankle: Some(v[15].into()),
            right_ankle: Some(v[16].into()),
            score: s,
        }
    }
}

// 3D
impl From<Pose2D> for (Vec<SPoint2>, f64) {
    fn from(pose: Pose2D) -> Self {
        // TODO: Allow missing points
        (
            vec![
                pose.nose.unwrap().into(),
                pose.left_eye.unwrap().into(),
                pose.right_eye.unwrap().into(),
                pose.left_ear.unwrap().into(),
                pose.right_ear.unwrap().into(),
                pose.left_shoulder.unwrap().into(),
                pose.right_shoulder.unwrap().into(),
                pose.left_elbow.unwrap().into(),
                pose.right_elbow.unwrap().into(),
                pose.left_wrist.unwrap().into(),
                pose.right_wrist.unwrap().into(),
                pose.left_hip.unwrap().into(),
                pose.right_hip.unwrap().into(),
                pose.left_knee.unwrap().into(),
                pose.right_knee.unwrap().into(),
                pose.left_ankle.unwrap().into(),
                pose.right_ankle.unwrap().into(),
            ],
            pose.score,
        )
    }
}

impl From<Pose3D> for (Vec<SPoint3>, f64) {
    fn from(pose: Pose3D) -> Self {
        // TODO: Allow missing points
        (
            vec![
                pose.nose.unwrap().into(),
                pose.left_eye.unwrap().into(),
                pose.right_eye.unwrap().into(),
                pose.left_ear.unwrap().into(),
                pose.right_ear.unwrap().into(),
                pose.left_shoulder.unwrap().into(),
                pose.right_shoulder.unwrap().into(),
                pose.left_elbow.unwrap().into(),
                pose.right_elbow.unwrap().into(),
                pose.left_wrist.unwrap().into(),
                pose.right_wrist.unwrap().into(),
                pose.left_hip.unwrap().into(),
                pose.right_hip.unwrap().into(),
                pose.left_knee.unwrap().into(),
                pose.right_knee.unwrap().into(),
                pose.left_ankle.unwrap().into(),
                pose.right_ankle.unwrap().into(),
            ],
            pose.score,
        )
    }
}

// Serialization for VRPN

impl IntoIterator for Point3D {
    type Item = f64;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        vec![self.x, self.y, self.z, self.score].into_iter()
    }
}

impl From<Vec<f64>> for Point3D {
    fn from(v: Vec<f64>) -> Self {
        Self {
            x: v[0],
            y: v[1],
            z: v[2],
            score: v[3],
        }
    }
}

impl From<Pose3D> for Vec<f64> {
    fn from(pose: Pose3D) -> Self {
        let mut values = Vec::<f64>::with_capacity(NUM_CHANNELS);
        values.extend(pose.nose.unwrap());
        values.extend(pose.left_eye.unwrap());
        values.extend(pose.right_eye.unwrap());
        values.extend(pose.left_ear.unwrap());
        values.extend(pose.right_ear.unwrap());
        values.extend(pose.left_shoulder.unwrap());
        values.extend(pose.right_shoulder.unwrap());
        values.extend(pose.left_elbow.unwrap());
        values.extend(pose.right_elbow.unwrap());
        values.extend(pose.left_wrist.unwrap());
        values.extend(pose.right_wrist.unwrap());
        values.extend(pose.left_hip.unwrap());
        values.extend(pose.right_hip.unwrap());
        values.extend(pose.left_knee.unwrap());
        values.extend(pose.right_knee.unwrap());
        values.extend(pose.left_ankle.unwrap());
        values.extend(pose.right_ankle.unwrap());
        values.extend(vec![pose.score]);
        assert_eq!(values.len(), NUM_CHANNELS);

        values
    }
}

impl From<Vec<f64>> for Pose3D {
    fn from(mut values: Vec<f64>) -> Self {
        Pose3D {
            nose: Some(pop_n(&mut values, NDIM).into()),
            left_eye: Some(pop_n(&mut values, NDIM).into()),
            right_eye: Some(pop_n(&mut values, NDIM).into()),
            left_ear: Some(pop_n(&mut values, NDIM).into()),
            right_ear: Some(pop_n(&mut values, NDIM).into()),
            left_shoulder: Some(pop_n(&mut values, NDIM).into()),
            right_shoulder: Some(pop_n(&mut values, NDIM).into()),
            left_elbow: Some(pop_n(&mut values, NDIM).into()),
            right_elbow: Some(pop_n(&mut values, NDIM).into()),
            left_wrist: Some(pop_n(&mut values, NDIM).into()),
            right_wrist: Some(pop_n(&mut values, NDIM).into()),
            left_hip: Some(pop_n(&mut values, NDIM).into()),
            right_hip: Some(pop_n(&mut values, NDIM).into()),
            left_knee: Some(pop_n(&mut values, NDIM).into()),
            right_knee: Some(pop_n(&mut values, NDIM).into()),
            left_ankle: Some(pop_n(&mut values, NDIM).into()),
            right_ankle: Some(pop_n(&mut values, NDIM).into()),
            score: values.pop().unwrap(),
        }
    }
}

// Random generation of points, poses, and images
impl Distribution<Point2D> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Point2D {
        Point2D {
            x: rng.gen(),
            y: rng.gen(),
            score: rng.gen(),
        }
    }
}

impl Distribution<Point3D> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Point3D {
        Point3D {
            x: rng.gen(),
            y: rng.gen(),
            z: rng.gen(),
            score: rng.gen(),
        }
    }
}

impl Distribution<Pose2D> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Pose2D {
        Pose2D {
            nose: Some(rng.gen()),
            left_eye: Some(rng.gen()),
            right_eye: Some(rng.gen()),
            left_ear: Some(rng.gen()),
            right_ear: Some(rng.gen()),
            left_shoulder: Some(rng.gen()),
            right_shoulder: Some(rng.gen()),
            left_elbow: Some(rng.gen()),
            right_elbow: Some(rng.gen()),
            left_wrist: Some(rng.gen()),
            right_wrist: Some(rng.gen()),
            left_hip: Some(rng.gen()),
            right_hip: Some(rng.gen()),
            left_knee: Some(rng.gen()),
            right_knee: Some(rng.gen()),
            left_ankle: Some(rng.gen()),
            right_ankle: Some(rng.gen()),
            score: rng.gen(),
        }
    }
}

impl Distribution<Pose3D> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Pose3D {
        Pose3D {
            nose: Some(rng.gen()),
            left_eye: Some(rng.gen()),
            right_eye: Some(rng.gen()),
            left_ear: Some(rng.gen()),
            right_ear: Some(rng.gen()),
            left_shoulder: Some(rng.gen()),
            right_shoulder: Some(rng.gen()),
            left_elbow: Some(rng.gen()),
            right_elbow: Some(rng.gen()),
            left_wrist: Some(rng.gen()),
            right_wrist: Some(rng.gen()),
            left_hip: Some(rng.gen()),
            right_hip: Some(rng.gen()),
            left_knee: Some(rng.gen()),
            right_knee: Some(rng.gen()),
            left_ankle: Some(rng.gen()),
            right_ankle: Some(rng.gen()),
            score: rng.gen(),
        }
    }
}

impl Distribution<ImageData> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> ImageData {
        let width: usize = 30;
        let height: usize = 10;
        let num_pixels = width * height;
        // Three channels: (R, G, B) for each pixel
        let num_bytes = 3 * num_pixels;

        ImageData {
            width: width.try_into().unwrap(),
            height: height.try_into().unwrap(),
            data: (0..num_bytes).map(|_| rng.gen()).collect(),
        }
    }
}
