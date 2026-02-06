//! Integration test for the HTTP MJPEG camera stream endpoint.
//!
//! Starts the HTTP server with a hub, GETs /api/camera/{group}/{name}/stream,
//! and asserts 200 OK and multipart Content-Type.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use posenet_vr_hub::grpc::proto::{CameraInfo, Snapshot};
use posenet_vr_hub::grpc::server::HubServer;
use posenet_vr_hub::http_server::{HttpConfig, HttpServer};

#[tokio::test]
async fn test_mjpeg_endpoint_returns_200_and_multipart() {
    let (cameras_tx, _cameras_rx) = mpsc::unbounded_channel::<CameraInfo>();
    let (snapshots_tx, _snapshots_rx) = mpsc::unbounded_channel::<Snapshot>();
    let hub = Arc::new(HubServer::new(cameras_tx, snapshots_tx, None));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let config = HttpConfig::new("127.0.0.1", port).unwrap();
    let server = HttpServer::new(config, hub, None);

    let server_handle = tokio::spawn(async move {
        let _ = server.run_with_listener(listener).await;
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    stream
        .write_all(
            b"GET /api/camera/default/cam1/stream HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    stream.flush().await.unwrap();

    let mut buf = [0u8; 1024];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        stream.read(&mut buf),
    )
    .await
    .expect("read timeout")
    .unwrap();
    // Response is headers (ASCII) then binary MJPEG; find end of headers.
    let header_end = buf[..n]
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(n);
    let headers = std::str::from_utf8(&buf[..header_end]).unwrap();

    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "expected 200 OK, got: {}",
        headers.lines().next().unwrap_or("")
    );
    assert!(
        headers.contains("multipart/x-mixed-replace"),
        "expected multipart Content-Type, got: {}",
        headers
    );

    server_handle.abort();
}
