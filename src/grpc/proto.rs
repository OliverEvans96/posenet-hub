tonic::include_proto!("posenet_vr");

use crate::utils::pop_n;

// PoseNet returns 17 points on the body
const NUM_KEYPOINTS: usize = 17;
const NDIM: usize = 3;
// (x,y,z) for 16 keypoints
const NUM_CHANNELS: usize = NDIM * NUM_KEYPOINTS;

// Convert between gRPC Point and nalgebra::Point

impl From<nalgebra::Point2<f64>> for Point2D {
    fn from(p: nalgebra::Point2<f64>) -> Self {
        Self { x: p.x, y: p.y, score: 1.0 }
    }
}

impl From<Point2D> for nalgebra::Point2<f64> {
    fn from(p: Point2D) -> Self {
        Self::new(p.x, p.y)
    }
}

impl From<nalgebra::Point3<f64>> for Point3D {
    fn from(p: nalgebra::Point3<f64>) -> Self {
        Self {
            x: p.x,
            y: p.y,
            z: p.z,
        }
    }
}

impl From<Point3D> for nalgebra::Point3<f64> {
    fn from(p: Point3D) -> Self {
        Self::new(p.x, p.y, p.z)
    }
}

// Convert between gRPC Pose and Vec<nalgebra::Point>

impl From<Vec<nalgebra::Point2<f64>>> for Pose2D {
    fn from(v: Vec<nalgebra::Point2<f64>>) -> Self {
        // TODO: Allow missing points
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
            score: 1.0
        }
    }
}

impl From<Vec<nalgebra::Point3<f64>>> for Pose3D {
    fn from(v: Vec<nalgebra::Point3<f64>>) -> Self {
        // TODO: Allow missing points
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
        }
    }
}

// 3D

impl From<Pose2D> for Vec<nalgebra::Point2<f64>> {
    fn from(pose: Pose2D) -> Self {
        // TODO: Allow missing points
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
        ]
    }
}

impl From<Pose3D> for Vec<nalgebra::Point3<f64>> {
    fn from(pose: Pose3D) -> Self {
        // TODO: Allow missing points
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
        ]
    }
}

// Serialization for VRPN

impl IntoIterator for Point3D {
    type Item = f64;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        vec![self.x, self.y, self.z].into_iter()
    }
}

impl From<Vec<f64>> for Point3D {
    fn from(v: Vec<f64>) -> Self {
        Self {
            x: v[0],
            y: v[1],
            z: v[2],
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
        }
    }
}
