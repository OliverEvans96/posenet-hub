tonic::include_proto!("posenet_vr");

// Convert between gRPC Point and nalgebra::Point

impl From<nalgebra::Point2<f64>> for Point2D {
    fn from(p: nalgebra::Point2<f64>) -> Self {
        Self { x: p.x, y: p.y }
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
