//! Crate error types. Library code uses these; application boundaries use `anyhow`.

use thiserror::Error;

// -----------------------------------------------------------------------------
// Missing data / invalid structure (library contract)
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum MissingField {
    #[error("Missing camera name.")]
    CameraName,
    #[error("Missing group name.")]
    GroupName,
    #[error("Camera info has no calibration data.")]
    Calibration,
    #[error("Calibration contains no intrinsics.")]
    Intrinsics,
    #[error("Calibration contains no extrinsics.")]
    Extrinsics,
    #[error("Snapshot missing timestamp.")]
    Timestamp,
    #[error("Missing keypoint: {0}")]
    Keypoint(String),
    #[error("Missing which_camera.")]
    WhichCamera,
    #[error("Pose/values length mismatch (expected score element).")]
    MissingScore,
}

#[derive(Debug, Error)]
pub enum CalculationError {
    #[error("Could not construct camera matrix")]
    CameraMatrixFailed(#[from] MissingField),
}

// -----------------------------------------------------------------------------
// OpenMVG / FFI / math (library)
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum OpenMvgError {
    #[error("FFI conversion failed (null or invalid pointer)")]
    FfiConversionFailed,
    #[error("Point was not homogeneous (e.g. zero w in projection)")]
    NonHomogeneousPoint,
    #[error("Dimension mismatch: points2d len {points} != camera_poses len {cameras}")]
    DimensionMismatch { points: usize, cameras: usize },
}

// -----------------------------------------------------------------------------
// I/O and channels (library → application boundary)
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("Channel send failed (receiver dropped)")]
    SendClosed,
    #[error("Channel recv failed (sender dropped)")]
    RecvClosed,
}

// -----------------------------------------------------------------------------
// Config / parsing (often used at application boundary)
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid address: {0}")]
    InvalidAddress(#[from] std::net::AddrParseError),
}

#[derive(Debug, Error)]
pub enum IoError {
    #[error("Image load failed: {0}")]
    Image(#[from] image::ImageError),
}

#[derive(Debug, Error)]
pub enum InvalidInput {
    #[error("Invalid input: {0}")]
    Message(String),
}

// -----------------------------------------------------------------------------
// Top-level crate error for library APIs that can fail in multiple ways
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum HubError {
    #[error(transparent)]
    MissingField(#[from] MissingField),
    #[error(transparent)]
    Calculation(#[from] CalculationError),
    #[error(transparent)]
    OpenMvg(#[from] OpenMvgError),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Io(#[from] IoError),
    #[error(transparent)]
    InvalidInput(#[from] InvalidInput),
}
