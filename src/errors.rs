use thiserror::Error;

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
}

#[derive(Debug, Error)]
pub enum CalculationError {
    #[error("Could not construct camera matrix")]
    CameraMatrixFailed(#[from] MissingField),
}
