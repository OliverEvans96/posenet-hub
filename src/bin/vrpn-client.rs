use posenet_vr_hub::vrpn::client::analog_listen;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device_name = "PoseNet0";
    let hostname = "tcp://localhost"; // using tcp to simplify connection when running in container

    println!("Start VRPN client");
    analog_listen(device_name, hostname);
    println!("VRPN client done");

    Ok(())
}
