//! Recording of timestamped [image + 2D pose] snapshots to a file.
//!
//! File format: magic "PNHR", version 1, then for each snapshot:
//! 4-byte little-endian length + protobuf Snapshot bytes.

use std::convert::TryInto;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;

use prost::Message;
use crate::grpc::proto::Snapshot;

/// Magic bytes at start of recording file.
pub const RECORDING_MAGIC: &[u8; 4] = b"PNHR";
/// Current file format version.
pub const RECORDING_VERSION: u8 = 1;

/// Result of stopping a recording.
#[derive(Debug, Clone)]
pub struct RecordingResult {
    /// Path where the file was saved.
    pub path: String,
    /// Number of snapshots written.
    pub frame_count: u64,
}

/// State of the recording subsystem.
pub struct RecordingState {
    /// If Some, we have an active recording.
    active: RwLock<Option<ActiveRecording>>,
    /// Base directory for recording files (e.g. from env POSENET_RECORDING_DIR).
    recording_dir: std::path::PathBuf,
    /// Path of last completed recording (for download). Filename only, not full path.
    last_completed_filename: RwLock<Option<String>>,
}

struct ActiveRecording {
    tx: mpsc::UnboundedSender<Snapshot>,
    group_filter: Option<String>,
    path: String,
    /// JoinHandle for the writer task; we await it on stop to get frame_count.
    join_handle: tokio::task::JoinHandle<Result<u64, RecordingError>>,
}

impl RecordingState {
    /// Create recording state. Uses `recording_dir` for new files; creates it if missing.
    pub fn new(recording_dir: std::path::PathBuf) -> Self {
        Self {
            active: RwLock::new(None),
            recording_dir,
            last_completed_filename: RwLock::new(None),
        }
    }

    /// Start recording. Optional group filter: if set, only snapshots from that group are written.
    /// Returns the full file path once started.
    pub async fn start_recording(
        &self,
        group_filter: Option<String>,
    ) -> Result<String, RecordingError> {
        let mut guard = self.active.write().await;
        if guard.is_some() {
            return Err(RecordingError::AlreadyRecording);
        }
        std::fs::create_dir_all(&self.recording_dir).map_err(RecordingError::Io)?;
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RecordingError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "system time before UNIX_EPOCH",
            )))?
            .as_secs();
        let filename = format!("recording_{}.pnhr", secs);
        let path = self.recording_dir.join(&filename);
        let path_string = path.to_string_lossy().into_owned();

        let (tx, mut rx) = mpsc::unbounded_channel::<Snapshot>();
        let path_for_task = path.clone();
        let join_handle = tokio::spawn(async move { run_writer(path_for_task, &mut rx).await });

        *guard = Some(ActiveRecording {
            tx,
            group_filter,
            path: path_string.clone(),
            join_handle,
        });
        Ok(path_string)
    }

    /// Stop recording. Returns the path and frame count. Waits briefly for the writer to finish.
    pub async fn stop_recording(&self) -> Result<RecordingResult, RecordingError> {
        let active = self.active.write().await.take();
        let Some(rec) = active else {
            return Err(RecordingError::NotRecording);
        };
        let path = rec.path.clone();
        drop(rec.tx);
        let frame_count: u64 = match timeout(Duration::from_secs(30), rec.join_handle).await {
            Ok(Ok(Ok(n))) => n,
            Ok(Ok(Err(_))) | Ok(Err(_)) | Err(_) => 0,
        };
        let filename = std::path::Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(String::from);
        if let Some(f) = filename {
            *self.last_completed_filename.write().await = Some(f);
        }
        Ok(RecordingResult {
            path: path.clone(),
            frame_count,
        })
    }

    /// Whether recording is currently active.
    pub async fn is_recording(&self) -> bool {
        self.active.read().await.is_some()
    }

    /// If recording is active and this snapshot matches the group filter, send it to the writer.
    pub async fn tee_snapshot(&self, snapshot: &Snapshot) {
        let guard = self.active.read().await;
        let Some(rec) = guard.as_ref() else {
            return;
        };
        if let Some(ref filter) = rec.group_filter {
            let group = snapshot
                .which_camera
                .as_ref()
                .map(|id| id.group_name.as_str())
                .unwrap_or("");
            if group != filter.as_str() {
                return;
            }
        }
        let _ = rec.tx.send(snapshot.clone());
    }

    /// Filename of the last completed recording (for download). None if none or cleared.
    pub async fn last_completed_filename(&self) -> Option<String> {
        self.last_completed_filename.read().await.clone()
    }

    /// Full path for a recording filename (for serving download).
    pub fn path_for_filename(&self, filename: &str) -> std::path::PathBuf {
        self.recording_dir.join(filename)
    }
}

async fn run_writer(
    path: std::path::PathBuf,
    rx: &mut mpsc::UnboundedReceiver<Snapshot>,
) -> Result<u64, RecordingError> {
    let mut file = std::fs::File::create(&path).map_err(RecordingError::Io)?;
    file.write_all(RECORDING_MAGIC).map_err(RecordingError::Io)?;
    file.write_all(&[RECORDING_VERSION]).map_err(RecordingError::Io)?;

    let mut frame_count: u64 = 0;
    while let Some(snapshot) = rx.recv().await {
        let bytes = prost::Message::encode_to_vec(&snapshot);
        let len = bytes.len() as u32;
        file.write_all(&len.to_le_bytes()).map_err(RecordingError::Io)?;
        file.write_all(&bytes).map_err(RecordingError::Io)?;
        frame_count += 1;
    }
    file.sync_all().map_err(RecordingError::Io)?;
    log::info!("Recording stopped: {} frames written to {:?}", frame_count, path);
    Ok(frame_count)
}

/// Read all snapshots from a recording file. Returns snapshots in file order.
pub fn read_recording(path: &std::path::Path) -> Result<Vec<Snapshot>, RecordingError> {
    let data = std::fs::read(path).map_err(RecordingError::Io)?;
    if data.len() < 5 {
        return Err(RecordingError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "recording file too short",
        )));
    }
    if &data[0..4] != RECORDING_MAGIC {
        return Err(RecordingError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid recording magic",
        )));
    }
    if data[4] != RECORDING_VERSION {
        return Err(RecordingError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported recording version",
        )));
    }
    let mut snapshots = Vec::new();
    let mut off = 5usize;
    while off + 4 <= data.len() {
        let len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + len > data.len() {
            return Err(RecordingError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "truncated snapshot in recording",
            )));
        }
        let snapshot = Snapshot::decode(bytes::Bytes::from(data[off..off + len].to_vec()))
            .map_err(|e| RecordingError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )))?;
        off += len;
        snapshots.push(snapshot);
    }
    Ok(snapshots)
}

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error("Recording already in progress")]
    AlreadyRecording,
    #[error("No recording in progress")]
    NotRecording,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::{CameraIdentifier, Image};
    use prost::Message;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_recording_write_and_read_back() {
        let dir = TempDir::new().unwrap();
        let state = RecordingState::new(dir.path().to_path_buf());
        state.start_recording(None).await.unwrap();

        let snap = Snapshot {
            timestamp: None,
            which_camera: Some(CameraIdentifier {
                group_name: "g".into(),
                camera_name: "c1".into(),
            }),
            poses: vec![],
            image: Some(Image {
                width: 2,
                height: 2,
                data: vec![0u8; 12],
            }),
        };
        state.tee_snapshot(&snap).await;
        state.tee_snapshot(&snap).await;

        let result = state.stop_recording().await.unwrap();
        assert_eq!(result.frame_count, 2);
        assert!(result.path.ends_with(".pnhr"));

        let data = std::fs::read(&result.path).unwrap();
        assert!(data.starts_with(RECORDING_MAGIC));
        assert_eq!(data[4], RECORDING_VERSION);
        let mut off = 5;
        for _ in 0..2 {
            let len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
            off += 4;
            let chunk: Vec<u8> = data[off..off + len].to_vec();
            let decoded = Snapshot::decode(bytes::Bytes::from(chunk)).unwrap();
            assert_eq!(decoded.which_camera.as_ref().unwrap().group_name, "g");
            off += len;
        }
        assert_eq!(off, data.len());
    }

    #[tokio::test]
    async fn test_recording_group_filter() {
        let dir = TempDir::new().unwrap();
        let state = RecordingState::new(dir.path().to_path_buf());
        state
            .start_recording(Some("group_a".into()))
            .await
            .unwrap();

        let snap_a = Snapshot {
            which_camera: Some(CameraIdentifier {
                group_name: "group_a".into(),
                camera_name: "c1".into(),
            }),
            ..Default::default()
        };
        let snap_b = Snapshot {
            which_camera: Some(CameraIdentifier {
                group_name: "group_b".into(),
                camera_name: "c1".into(),
            }),
            ..Default::default()
        };
        state.tee_snapshot(&snap_a).await;
        state.tee_snapshot(&snap_b).await;
        state.tee_snapshot(&snap_a).await;

        let result = state.stop_recording().await.unwrap();
        assert_eq!(result.frame_count, 2);
    }

    #[tokio::test]
    async fn test_recording_double_start_fails() {
        let dir = TempDir::new().unwrap();
        let state = RecordingState::new(dir.path().to_path_buf());
        state.start_recording(None).await.unwrap();
        let r = state.start_recording(None).await;
        assert!(matches!(r, Err(RecordingError::AlreadyRecording)));
        state.stop_recording().await.unwrap();
    }

    #[tokio::test]
    async fn test_recording_stop_without_start_fails() {
        let dir = TempDir::new().unwrap();
        let state = RecordingState::new(dir.path().to_path_buf());
        let r = state.stop_recording().await;
        assert!(matches!(r, Err(RecordingError::NotRecording)));
    }

    #[test]
    fn test_read_recording_invalid_magic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.pnhr");
        std::fs::write(&path, b"XXXX").unwrap();
        let r = read_recording(&path);
        assert!(r.is_err());
    }

    #[test]
    fn test_read_recording_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("empty.pnhr");
        std::fs::write(&path, &[b'P', b'N', b'H', b'R', 1]).unwrap();
        let snapshots = read_recording(&path).unwrap();
        assert!(snapshots.is_empty());
    }
}
