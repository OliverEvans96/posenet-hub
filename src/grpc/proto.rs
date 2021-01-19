use nalgebra::Point3;

tonic::include_proto!("posenet_vr");

impl From<Vec<Point3<f64>>> for Pose3D {
    fn from(v: Vec<Point3<f64>>) -> Self {
        Self {
            nose: Some(Point3D {
                x: v[0].x,
                y: v[0].y,
                z: v[0].z,
            }),
            left_eye: Some(Point3D {
                x: v[1].x,
                y: v[1].y,
                z: v[1].z,
            }),
            right_eye: Some(Point3D {
                x: v[2].x,
                y: v[2].y,
                z: v[2].z,
            }),
            left_ear: Some(Point3D {
                x: v[3].x,
                y: v[3].y,
                z: v[3].z,
            }),
            right_ear: Some(Point3D {
                x: v[4].x,
                y: v[4].y,
                z: v[4].z,
            }),
            left_shoulder: Some(Point3D {
                x: v[5].x,
                y: v[5].y,
                z: v[5].z,
            }),
            right_shoulder: Some(Point3D {
                x: v[6].x,
                y: v[6].y,
                z: v[6].z,
            }),
            left_elbow: Some(Point3D {
                x: v[7].x,
                y: v[7].y,
                z: v[7].z,
            }),
            right_elbow: Some(Point3D {
                x: v[8].x,
                y: v[8].y,
                z: v[8].z,
            }),
            left_wrist: Some(Point3D {
                x: v[9].x,
                y: v[9].y,
                z: v[9].z,
            }),
            right_wrist: Some(Point3D {
                x: v[10].x,
                y: v[10].y,
                z: v[10].z,
            }),
            left_hip: Some(Point3D {
                x: v[11].x,
                y: v[11].y,
                z: v[11].z,
            }),
            right_hip: Some(Point3D {
                x: v[12].x,
                y: v[12].y,
                z: v[12].z,
            }),
            left_knee: Some(Point3D {
                x: v[13].x,
                y: v[13].y,
                z: v[13].z,
            }),
            right_knee: Some(Point3D {
                x: v[14].x,
                y: v[14].y,
                z: v[14].z,
            }),
            left_ankle: Some(Point3D {
                x: v[15].x,
                y: v[15].y,
                z: v[15].z,
            }),
            right_ankle: Some(Point3D {
                x: v[16].x,
                y: v[16].y,
                z: v[16].z,
            }),
        }
    }
}
