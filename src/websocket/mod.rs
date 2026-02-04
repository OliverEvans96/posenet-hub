//! WebSocket server that streams 3D pose data to connected clients.
//!
//! Subscribes to the same broadcast channel as the VRPN server and sends
//! pose updates as JSON over WebSocket.

mod server;

pub use server::{
    Pose3DJson, Point3DJson, PoseStreamMessage, WebSocketConfig, WebSocketServer,
};
