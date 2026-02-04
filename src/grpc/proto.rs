tonic::include_proto!("posenet_vr");

use rand::{distributions::Standard, prelude::Distribution};
use std::convert::TryFrom;
use std::path::Path;
use uuid::Uuid;

use crate::errors::{HubError, IoError, MissingField};
use crate::utils::pop_n;

// PoseNet returns 17 points on the body
const NUM_KEYPOINTS: usize = 17;
const NDIM: usize = 4;
// (x,y,z, score) for 17 keypoints + pose score
const NUM_CHANNELS: usize = NDIM * NUM_KEYPOINTS + 1;

impl SessionToken {
    pub fn new() -> Self {
        Self {
            data: Uuid::new_v4().to_string(),
        }
    }
}

impl CommandToken {
    pub fn new() -> Self {
        Self {
            data: Uuid::new_v4().to_string(),
        }
    }
}

pub enum CommandResponseMessage {
    Begin,
    Data(CommandResponse),
}

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

// TODO: Change most of these to TryFrom

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

// Pose keypoint names for error messages
const POSE_KEYPOINTS: [&str; 17] = [
    "nose", "left_eye", "right_eye", "left_ear", "right_ear",
    "left_shoulder", "right_shoulder", "left_elbow", "right_elbow",
    "left_wrist", "right_wrist", "left_hip", "right_hip",
    "left_knee", "right_knee", "left_ankle", "right_ankle",
];

fn pose2d_to_spoints(pose: &Pose2D) -> Result<(Vec<SPoint2>, f64), MissingField> {
    let opts = [
        pose.nose.as_ref(),
        pose.left_eye.as_ref(),
        pose.right_eye.as_ref(),
        pose.left_ear.as_ref(),
        pose.right_ear.as_ref(),
        pose.left_shoulder.as_ref(),
        pose.right_shoulder.as_ref(),
        pose.left_elbow.as_ref(),
        pose.right_elbow.as_ref(),
        pose.left_wrist.as_ref(),
        pose.right_wrist.as_ref(),
        pose.left_hip.as_ref(),
        pose.right_hip.as_ref(),
        pose.left_knee.as_ref(),
        pose.right_knee.as_ref(),
        pose.left_ankle.as_ref(),
        pose.right_ankle.as_ref(),
    ];
    let mut out = Vec::with_capacity(17);
    for (i, opt) in opts.iter().enumerate() {
        let p = opt
            .ok_or_else(|| MissingField::Keypoint(POSE_KEYPOINTS[i].to_string()))?;
        out.push(p.clone().into());
    }
    Ok((out, pose.score))
}

fn pose3d_to_spoints(pose: &Pose3D) -> Result<(Vec<SPoint3>, f64), MissingField> {
    let opts = [
        pose.nose.as_ref(),
        pose.left_eye.as_ref(),
        pose.right_eye.as_ref(),
        pose.left_ear.as_ref(),
        pose.right_ear.as_ref(),
        pose.left_shoulder.as_ref(),
        pose.right_shoulder.as_ref(),
        pose.left_elbow.as_ref(),
        pose.right_elbow.as_ref(),
        pose.left_wrist.as_ref(),
        pose.right_wrist.as_ref(),
        pose.left_hip.as_ref(),
        pose.right_hip.as_ref(),
        pose.left_knee.as_ref(),
        pose.right_knee.as_ref(),
        pose.left_ankle.as_ref(),
        pose.right_ankle.as_ref(),
    ];
    let mut out = Vec::with_capacity(17);
    for (i, opt) in opts.iter().enumerate() {
        let p = opt
            .ok_or_else(|| MissingField::Keypoint(POSE_KEYPOINTS[i].to_string()))?;
        out.push(p.clone().into());
    }
    Ok((out, pose.score))
}

impl TryFrom<Pose2D> for (Vec<SPoint2>, f64) {
    type Error = MissingField;
    fn try_from(pose: Pose2D) -> Result<Self, Self::Error> {
        pose2d_to_spoints(&pose)
    }
}

impl TryFrom<Pose3D> for (Vec<SPoint3>, f64) {
    type Error = MissingField;
    fn try_from(pose: Pose3D) -> Result<Self, Self::Error> {
        pose3d_to_spoints(&pose)
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

impl TryFrom<Pose3D> for Vec<f64> {
    type Error = MissingField;
    fn try_from(pose: Pose3D) -> Result<Self, Self::Error> {
        let (points, score) = pose3d_to_spoints(&pose)?;
        let mut values = Vec::<f64>::with_capacity(NUM_CHANNELS);
        for p in points {
            values.extend([p.0.x, p.0.y, p.0.z, p.1]);
        }
        values.push(score);
        Ok(values)
    }
}

impl TryFrom<Vec<f64>> for Pose3D {
    type Error = MissingField;
    fn try_from(mut values: Vec<f64>) -> Result<Self, Self::Error> {
        if values.len() < NUM_CHANNELS {
            return Err(MissingField::MissingScore);
        }
        let score = values.pop().ok_or(MissingField::MissingScore)?;
        // pop_n takes from the end, so first pop_n is right_ankle, last is nose
        let right_ankle = pop_n(&mut values, NDIM);
        let left_ankle = pop_n(&mut values, NDIM);
        let right_knee = pop_n(&mut values, NDIM);
        let left_knee = pop_n(&mut values, NDIM);
        let right_hip = pop_n(&mut values, NDIM);
        let left_hip = pop_n(&mut values, NDIM);
        let right_wrist = pop_n(&mut values, NDIM);
        let left_wrist = pop_n(&mut values, NDIM);
        let right_elbow = pop_n(&mut values, NDIM);
        let left_elbow = pop_n(&mut values, NDIM);
        let right_shoulder = pop_n(&mut values, NDIM);
        let left_shoulder = pop_n(&mut values, NDIM);
        let right_ear = pop_n(&mut values, NDIM);
        let left_ear = pop_n(&mut values, NDIM);
        let right_eye = pop_n(&mut values, NDIM);
        let left_eye = pop_n(&mut values, NDIM);
        let nose = pop_n(&mut values, NDIM);
        Ok(Pose3D {
            nose: Some(Point3D::from(nose)),
            left_eye: Some(Point3D::from(left_eye)),
            right_eye: Some(Point3D::from(right_eye)),
            left_ear: Some(Point3D::from(left_ear)),
            right_ear: Some(Point3D::from(right_ear)),
            left_shoulder: Some(Point3D::from(left_shoulder)),
            right_shoulder: Some(Point3D::from(right_shoulder)),
            left_elbow: Some(Point3D::from(left_elbow)),
            right_elbow: Some(Point3D::from(right_elbow)),
            left_wrist: Some(Point3D::from(left_wrist)),
            right_wrist: Some(Point3D::from(right_wrist)),
            left_hip: Some(Point3D::from(left_hip)),
            right_hip: Some(Point3D::from(right_hip)),
            left_knee: Some(Point3D::from(left_knee)),
            right_knee: Some(Point3D::from(right_knee)),
            left_ankle: Some(Point3D::from(left_ankle)),
            right_ankle: Some(Point3D::from(right_ankle)),
            score,
        })
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

impl From<image::DynamicImage> for Image {
    fn from(img: image::DynamicImage) -> Self {
        let rgb_img = img.to_rgb8();
        let (width, height) = rgb_img.dimensions();
        Self {
            width,
            height,
            data: rgb_img.into_raw(),
        }
    }
}

impl Image {
    /// Read image from file
    pub fn from_path(image_path: &Path) -> Result<Self, HubError> {
        let img = image::open(image_path)
            .map_err(IoError::from)
            .map_err(HubError::from)?;
        Ok(img.into())
    }
}

impl Distribution<Image> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Image {
        const W: u32 = 30;
        const H: u32 = 10;
        let num_bytes = (W * H * 3) as usize;
        Image {
            width: W,
            height: H,
            data: (0..num_bytes).map(|_| rng.gen()).collect(),
        }
    }
}

pub struct CameraUniqueIdentifier {
    pub group_name: String,
    pub camera_name: String,
}

impl TryFrom<CameraIdentifier> for CameraUniqueIdentifier {
    type Error = MissingField;

    fn try_from(value: CameraIdentifier) -> Result<Self, Self::Error> {
        let CameraIdentifier {
            group_name,
            camera_name,
        } = value;
        if camera_name.is_empty() {
            Err(MissingField::CameraName)
        } else if group_name.is_empty() {
            Err(MissingField::GroupName)
        } else {
            Ok(Self {
                group_name,
                camera_name,
            })
        }
    }
}

impl From<CameraUniqueIdentifier> for CameraIdentifier {
    fn from(value: CameraUniqueIdentifier) -> Self {
        Self {
            group_name: value.group_name,
            camera_name: value.camera_name,
        }
    }
}
