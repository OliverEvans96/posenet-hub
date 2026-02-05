//! HTTP server that serves MJPEG streams for individual cameras from the hub's stream cache.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hyper::body::{Body, Bytes};
use hyper::server::conn::Http;
use hyper::service::service_fn;
use hyper::{Request, Response};
use tokio::net::TcpListener;
use tokio::time::interval;

use crate::grpc::proto::Image;
use crate::grpc::server::HubServer;

/// Default MJPEG stream frame rate (frames per second).
const MJPEG_FPS: u32 = 10;
/// JPEG quality (0-100).
const JPEG_QUALITY: u8 = 85;
/// Multipart boundary for MJPEG.
const BOUNDARY: &str = "frame";

/// Encodes raw RGB image bytes to JPEG. Returns None if the image is invalid or encoding fails.
fn image_to_jpeg(img: &Image) -> Option<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ImageEncoder;

    let w = img.width;
    let h = img.height;
    if w == 0 || h == 0 {
        return None;
    }
    let expected = (w as usize)
        .checked_mul(h as usize)?
        .checked_mul(3)?;
    if img.data.len() != expected {
        return None;
    }
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    encoder
        .write_image(&img.data, w, h, image::ColorType::Rgb8)
        .ok()?;
    Some(out)
}

/// Returns a minimal valid JPEG (1x1 black pixel). Used to send an initial frame
/// so response headers are flushed before real frames arrive.
fn placeholder_jpeg() -> Vec<u8> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ImageEncoder;
    let w = 1u32;
    let h = 1u32;
    let data = [0u8; 3];
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    encoder.write_image(&data, w, h, image::ColorType::Rgb8).unwrap();
    out
}

/// Builds one MJPEG part: boundary + headers + jpeg bytes.
fn mjpeg_part(jpeg: &[u8]) -> Vec<u8> {
    let header = format!(
        "--{}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
        BOUNDARY,
        jpeg.len()
    );
    let mut part = header.into_bytes();
    part.extend_from_slice(jpeg);
    part.extend_from_slice(b"\r\n");
    part
}

/// Path prefix for camera stream API.
pub const CAMERA_STREAM_PATH_PREFIX: &str = "/api/camera/";

/// Parses path like "/api/camera/{group}/{name}/stream" into (group, name).
/// Group and name are URL-decoded. Returns None if path does not match.
pub fn parse_camera_stream_path(path: &str) -> Option<(String, String)> {
    let path = path.strip_prefix(CAMERA_STREAM_PATH_PREFIX)?;
    let path = path.strip_suffix("/stream")?;
    let mut segments = path.splitn(2, '/');
    let group = segments.next()?;
    let name = segments.next()?;
    if group.is_empty() || name.is_empty() {
        return None;
    }
    let group = percent_encoding::percent_decode_str(group).decode_utf8().ok()?.into_owned();
    let name = percent_encoding::percent_decode_str(name).decode_utf8().ok()?.into_owned();
    Some((group, name))
}

/// Service handler: GET /api/camera/{group}/{name}/stream returns MJPEG stream.
async fn handle_request(
    hub: Arc<HubServer>,
    req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
    if req.method() != hyper::Method::GET {
        return Ok(Response::builder()
            .status(405)
            .body(Body::from("Method Not Allowed"))
            .unwrap());
    }
    let path = req.uri().path();
    let Some((group, name)) = parse_camera_stream_path(path) else {
        return Ok(Response::builder()
            .status(404)
            .body(Body::from("Not Found"))
            .unwrap());
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Bytes, Box<dyn std::error::Error + Send + Sync>>>();
    let hub_clone = hub.clone();
    tokio::spawn(async move {
        // Send one placeholder frame immediately so response headers are flushed.
        let placeholder = mjpeg_part(&placeholder_jpeg());
        let _ = tx.send(Ok(Bytes::from(placeholder)));
        let mut ticker = interval(Duration::from_millis(1000 / MJPEG_FPS as u64));
        let mut frames_sent: u64 = 0;
        loop {
            ticker.tick().await;
            let snapshot = hub_clone.get_latest_snapshot(&group, &name);
            let Some(snapshot) = snapshot else {
                log::debug!("MJPEG {}: no snapshot in cache", name);
                continue;
            };
            let Some(ref img) = snapshot.image else {
                continue;
            };
            // Encode in a blocking thread so we don't starve the async runtime (gRPC stream processing).
            let img = img.clone();
            let jpeg = match tokio::task::spawn_blocking(move || image_to_jpeg(&img)).await {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("MJPEG {}: spawn_blocking join error: {}", name, e);
                    continue;
                }
            };
            let part = mjpeg_part(&jpeg);
            if tx.send(Ok(Bytes::from(part))).is_err() {
                break;
            }
            frames_sent += 1;
            if frames_sent % 60 == 0 {
                log::debug!("MJPEG {}: {} frames sent", name, frames_sent);
            }
        }
    });
    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    let boxed: Box<dyn futures_util::Stream<Item = Result<Bytes, Box<dyn std::error::Error + Send + Sync>>> + Send> =
        Box::new(stream);
    let body = Body::from(boxed);
    let response = Response::builder()
        .status(200)
        .header(
            hyper::header::CONTENT_TYPE,
            format!("multipart/x-mixed-replace; boundary={}", BOUNDARY),
        )
        .body(body)
        .unwrap();
    Ok(response)
}

/// HTTP server configuration.
pub struct HttpConfig {
    pub addr: SocketAddr,
}

impl HttpConfig {
    pub fn new(ip: &str, port: u16) -> anyhow::Result<Self> {
        Ok(Self {
            addr: format!("{}:{}", ip, port).parse()?,
        })
    }

    pub fn try_default() -> anyhow::Result<Self> {
        Self::new("0.0.0.0", 9002)
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self::try_default().expect("default HTTP config 0.0.0.0:9002 must be valid")
    }
}

/// HTTP server that serves MJPEG camera streams.
pub struct HttpServer {
    config: HttpConfig,
    hub: Arc<HubServer>,
}

impl HttpServer {
    pub fn new(config: HttpConfig, hub: Arc<HubServer>) -> Self {
        Self { config, hub }
    }

    /// Run the server with the given listener (for tests; use port 0 for a random port).
    pub async fn run_with_listener(self, listener: TcpListener) -> anyhow::Result<()> {
        log::info!(
            "HTTP MJPEG camera streams listening on http://{}",
            listener.local_addr()?
        );
        let hub = self.hub;
        loop {
            let (stream, _) = listener.accept().await?;
            let hub = hub.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req| {
                    let hub = hub.clone();
                    async move { handle_request(hub, req).await }
                });
                if let Err(e) = Http::new().serve_connection(stream, service).await {
                    log::debug!("HTTP connection error: {}", e);
                }
            });
        }
    }

    /// Run the server. Blocks until the listener is closed or an error occurs.
    pub async fn run(self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.config.addr).await?;
        self.run_with_listener(listener).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_camera_stream_path() {
        assert_eq!(
            parse_camera_stream_path("/api/camera/default/cam1/stream"),
            Some(("default".into(), "cam1".into()))
        );
        assert_eq!(
            parse_camera_stream_path("/api/camera/grp/camera-one/stream"),
            Some(("grp".into(), "camera-one".into()))
        );
        assert!(parse_camera_stream_path("/api/camera/grp/stream").is_none());
        assert!(parse_camera_stream_path("/api/camera/grp/").is_none());
        assert!(parse_camera_stream_path("/other").is_none());
    }

    #[test]
    fn test_image_to_jpeg() {
        let img = Image {
            width: 2,
            height: 2,
            data: vec![0u8; 2 * 2 * 3],
        };
        let jpeg = image_to_jpeg(&img).unwrap();
        assert!(!jpeg.is_empty());
    }

    #[test]
    fn test_image_to_jpeg_invalid() {
        let img = Image {
            width: 2,
            height: 2,
            data: vec![0u8; 5],
        };
        assert!(image_to_jpeg(&img).is_none());
    }

    #[test]
    fn test_mjpeg_part() {
        let jpeg = vec![0xff, 0xd8, 0xff];
        let part = mjpeg_part(&jpeg);
        assert!(part.starts_with(b"--frame\r\n"));
        assert!(part.windows(b"Content-Type: image/jpeg".len()).any(|w| w == b"Content-Type: image/jpeg"));
        assert!(part.windows(b"Content-Length: 3".len()).any(|w| w == b"Content-Length: 3"));
        assert!(part.ends_with(b"\r\n"));
    }

    #[test]
    fn test_placeholder_jpeg() {
        let jpeg = placeholder_jpeg();
        assert!(!jpeg.is_empty());
        assert!(jpeg.starts_with(&[0xff, 0xd8, 0xff]), "valid JPEG SOI");
    }
}
