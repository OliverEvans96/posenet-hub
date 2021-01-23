use posenet_vr_hub::vrpn::client::tracker_listen;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device_name = "Tracker0";
    let hostname = "localhost";

    println!("Start VRPN client");
    tracker_listen(device_name, hostname);
    println!("VRPN client done");

    Ok(())
}
