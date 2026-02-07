//! TOML-based configuration for the hub server.
//!
//! Load with `HubConfig::load_path(path)` or `HubConfig::load_toml(str)`.
//! Environment variable `POSENET_RECORDING_DIR` overrides `recording.dir` when set.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::grpc::server::GrpcConfig;
use crate::http_server::HttpConfig;
use crate::triangulator::{SmoothingConfig, TriangulatorConfig};
use crate::vrpn::server::VrpnConfig;
use crate::websocket::WebSocketConfig;

// -----------------------------------------------------------------------------
// TOML-friendly structs (all optional with defaults)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct GrpcConfigToml {
    pub bind: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct HttpConfigToml {
    pub bind: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct WebSocketConfigToml {
    pub enabled: Option<bool>,
    pub bind: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct VrpnConfigToml {
    pub enabled: Option<bool>,
    pub bind: Option<String>,
    pub port: Option<u16>,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct RecordingConfigToml {
    pub dir: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct TriangulatorConfigToml {
    pub pose_expiration_ms: Option<u64>,
    pub poll_interval_ms: Option<u64>,
    pub min_cameras: Option<usize>,
    pub pose_matching_threshold_px: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct SmoothingConfigToml {
    /// If false, 2D smoothing is disabled (raw poses used for matching and camera_views).
    pub enabled: Option<bool>,
    /// When true, use position-only Kalman (no velocity in state). Current implementation is always position-only; this is for future constant-velocity model.
    pub position_only: Option<bool>,
    pub r0: Option<f64>,
    pub score_epsilon: Option<f64>,
    pub process_noise: Option<f64>,
    pub assoc_threshold_px: Option<f64>,
    pub min_score_observed: Option<f64>,
    pub hold_frames: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct HubConfigToml {
    pub grpc: Option<GrpcConfigToml>,
    pub http: Option<HttpConfigToml>,
    pub websocket: Option<WebSocketConfigToml>,
    pub vrpn: Option<VrpnConfigToml>,
    pub recording: Option<RecordingConfigToml>,
    pub triangulator: Option<TriangulatorConfigToml>,
    pub smoothing: Option<SmoothingConfigToml>,
}

// -----------------------------------------------------------------------------
// Resolved config (after merging with defaults)
// -----------------------------------------------------------------------------

/// Resolved hub configuration ready to build servers and controller.
#[derive(Debug, Clone)]
pub struct HubConfig {
    pub grpc: GrpcConfig,
    pub http: HttpConfig,
    pub websocket: WebSocketConfig,
    pub websocket_enabled: bool,
    pub vrpn: VrpnConfig,
    pub vrpn_enabled: bool,
    pub recording_dir: std::path::PathBuf,
    pub triangulator: TriangulatorConfig,
}

impl HubConfig {
    /// Load config from a TOML file. Missing file is an error.
    pub fn load_path(path: &Path) -> anyhow::Result<Self> {
        let s = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read config {}: {}", path.display(), e))?;
        Self::load_toml(&s)
    }

    /// Load config from TOML string. Merges with defaults; missing sections are defaulted.
    pub fn load_toml(toml_str: &str) -> anyhow::Result<Self> {
        let raw: HubConfigToml = toml::from_str(toml_str)
            .map_err(|e| anyhow::anyhow!("Invalid config TOML: {}", e))?;
        Self::from_toml(raw)
    }

    /// Build resolved config from parsed TOML. Env POSENET_RECORDING_DIR overrides recording.dir.
    pub fn from_toml(raw: HubConfigToml) -> anyhow::Result<Self> {
        let grpc = raw.grpc.clone().unwrap_or_default();
        let bind_grpc = grpc.bind.as_deref().unwrap_or("0.0.0.0");
        let port_grpc = grpc.port.unwrap_or(50051);
        let grpc_config = GrpcConfig::new(bind_grpc, port_grpc)
            .map_err(|e| anyhow::anyhow!("grpc: {}", e))?;

        let http = raw.http.clone().unwrap_or_default();
        let bind_http = http.bind.as_deref().unwrap_or("0.0.0.0");
        let port_http = http.port.unwrap_or(9002);
        let http_config = HttpConfig::new(bind_http, port_http)
            .map_err(|e| anyhow::anyhow!("http: {}", e))?;

        let ws = raw.websocket.clone().unwrap_or_default();
        let websocket_enabled = ws.enabled.unwrap_or(false);
        let bind_ws = ws.bind.as_deref().unwrap_or("0.0.0.0");
        let port_ws = ws.port.unwrap_or(9001);
        let websocket_config = WebSocketConfig::new(bind_ws, port_ws)
            .map_err(|e| anyhow::anyhow!("websocket: {}", e))?;

        let vrpn = raw.vrpn.clone().unwrap_or_default();
        let vrpn_enabled = vrpn.enabled.unwrap_or(false);
        let bind_vrpn = vrpn.bind.as_deref().unwrap_or("0.0.0.0");
        let port_vrpn = vrpn.port.unwrap_or(3883);
        let device_name = vrpn.device_name.as_deref().unwrap_or("PoseNet0");
        let vrpn_config = VrpnConfig::new(device_name, bind_vrpn, port_vrpn)
            .map_err(|e| anyhow::anyhow!("vrpn: {}", e))?;

        let recording_dir = std::env::var("POSENET_RECORDING_DIR")
            .ok()
            .or_else(|| raw.recording.as_ref().and_then(|r| r.dir.clone()))
            .unwrap_or_else(|| "recordings".to_string())
            .into();

        let tri_toml = raw.triangulator.clone().unwrap_or_default();
        let smoothing_toml = raw.smoothing.clone().unwrap_or_default();

        let pose_expiration = Duration::from_millis(tri_toml.pose_expiration_ms.unwrap_or(100));
        let poll_interval = Duration::from_millis(tri_toml.poll_interval_ms.unwrap_or(16));
        let min_cameras = tri_toml.min_cameras.unwrap_or(2);
        let pose_matching_threshold_px = tri_toml.pose_matching_threshold_px.unwrap_or(50.0);

        let smoothing_enabled = smoothing_toml.enabled.unwrap_or(true);
        let smoothing = if smoothing_enabled {
            Some(SmoothingConfig {
                r0: smoothing_toml.r0.unwrap_or(100.0),
                score_epsilon: smoothing_toml.score_epsilon.unwrap_or(0.01),
                process_noise: smoothing_toml.process_noise.unwrap_or(4.0),
                assoc_threshold_px: smoothing_toml.assoc_threshold_px.unwrap_or(120.0),
                min_score_observed: smoothing_toml.min_score_observed.unwrap_or(0.2),
                hold_frames: smoothing_toml.hold_frames.unwrap_or(1),
                position_only: smoothing_toml.position_only.unwrap_or(true),
            })
        } else {
            None
        };

        let triangulator = TriangulatorConfig {
            pose_expiration,
            poll_interval,
            min_cameras,
            pose_matching_threshold_px: Some(pose_matching_threshold_px),
            smoothing,
        };

        Ok(Self {
            grpc: grpc_config,
            http: http_config,
            websocket: websocket_config,
            websocket_enabled,
            vrpn: vrpn_config,
            vrpn_enabled,
            recording_dir,
            triangulator,
        })
    }

    /// Default config (no file). Uses same defaults as from_toml with empty HubConfigToml.
    pub fn default_resolved() -> anyhow::Result<Self> {
        Self::from_toml(HubConfigToml::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_uses_defaults() {
        let cfg = HubConfig::load_toml("").unwrap();
        assert_eq!(cfg.websocket_enabled, false);
        assert_eq!(cfg.vrpn_enabled, false);
        assert_eq!(cfg.triangulator.pose_expiration, Duration::from_millis(100));
        assert_eq!(cfg.triangulator.poll_interval, Duration::from_millis(16));
        assert_eq!(cfg.triangulator.min_cameras, 2);
        assert_eq!(
            cfg.triangulator.pose_matching_threshold_px,
            Some(50.0)
        );
        assert!(cfg.triangulator.smoothing.is_some());
        let s = cfg.triangulator.smoothing.unwrap();
        assert_eq!(s.r0, 100.0);
        assert_eq!(s.hold_frames, 1);
    }

    #[test]
    fn minimal_grpc_override() {
        let cfg = HubConfig::load_toml(
            r#"
[grpc]
port = 50052
"#,
        )
            .unwrap();
        // We can't easily assert GrpcConfig addr without exposing it; just ensure no error
        assert!(!cfg.websocket_enabled);
    }

    #[test]
    fn full_toml_roundtrip() {
        let toml = r#"
[grpc]
bind = "127.0.0.1"
port = 50051

[http]
bind = "0.0.0.0"
port = 9002

[websocket]
enabled = true
bind = "0.0.0.0"
port = 9001

[vrpn]
enabled = true
bind = "0.0.0.0"
port = 3883
device_name = "Tracker0"

[recording]
dir = "/var/posenet/recordings"

[triangulator]
pose-expiration-ms = 150
poll-interval-ms = 20
min-cameras = 3
pose-matching-threshold-px = 60.0

[smoothing]
r0 = 80.0
score-epsilon = 0.02
process-noise = 2.0
assoc-threshold-px = 100.0
min-score-observed = 0.3
hold-frames = 2
"#;
        let cfg = HubConfig::load_toml(toml).unwrap();
        assert_eq!(cfg.websocket_enabled, true);
        assert_eq!(cfg.vrpn_enabled, true);
        assert_eq!(cfg.triangulator.pose_expiration, Duration::from_millis(150));
        assert_eq!(cfg.triangulator.poll_interval, Duration::from_millis(20));
        assert_eq!(cfg.triangulator.min_cameras, 3);
        assert_eq!(
            cfg.triangulator.pose_matching_threshold_px,
            Some(60.0)
        );
        let s = cfg.triangulator.smoothing.unwrap();
        assert_eq!(s.r0, 80.0);
        assert_eq!(s.score_epsilon, 0.02);
        assert_eq!(s.process_noise, 2.0);
        assert_eq!(s.assoc_threshold_px, 100.0);
        assert_eq!(s.min_score_observed, 0.3);
        assert_eq!(s.hold_frames, 2);
        assert!(cfg.recording_dir.to_string_lossy().contains("recordings"));
    }

    #[test]
    fn invalid_toml_fails() {
        assert!(HubConfig::load_toml("not valid toml ???").is_err());
    }

    #[test]
    fn invalid_grpc_port_fails() {
        // port 99999 is out of range for u16 in some interpretations; use invalid bind
        let r = HubConfig::load_toml(
            r#"
[grpc]
bind = "999.999.999.999"
port = 50051
"#,
        );
        assert!(r.is_err());
    }

    #[test]
    fn default_resolved_succeeds() {
        let cfg = HubConfig::default_resolved().unwrap();
        assert_eq!(cfg.triangulator.min_cameras, 2);
    }
}
