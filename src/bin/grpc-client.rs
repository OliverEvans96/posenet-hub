use tonic::Request;

use proto::hub_service_client::HubServiceClient;
use proto::{CameraExtrinsics, CameraInfo, CameraIntrinsics, EulerAngles, Point3D};

pub mod proto {
    tonic::include_proto!("posenet_vr");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "http://[::1]:50051";

    let camera_info = CameraInfo {
        intrinsics: Some(CameraIntrinsics {}),
        extrinsics: Some(CameraExtrinsics {
            position: Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            orientation: Some(EulerAngles {
                yaw: 0.0,
                pitch: 0.0,
                roll: 0.0,
            }),
        }),
    };
    println!("Camera Info: {:?}", camera_info);

    let mut client = HubServiceClient::connect(addr).await?;
    let request = Request::new(camera_info);
    let response_promise = client.hello(request);
    println!("Message sent.");

    let response = response_promise.await?;
    let message = response.into_inner();
    println!("Reply received: {:?}", message);
    Ok(())
}
