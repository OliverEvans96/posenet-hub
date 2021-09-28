use structopt::StructOpt;

use posenet_vr_hub::vrpn::client::analog_listen;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "vrpn-client",
    version = "0.1.0",
    about = "Test vrpn-client receives 3D poses from posenet hub-server."
)]
struct VrpnClientCommand {
    /// Server address
    #[structopt(short, long, default_value = "localhost")]
    server: String,

    /// VRPN port
    #[structopt(short, long, default_value = "3883")]
    port: u8,

    /// VRPN device name
    #[structopt(short, long, default_value = "grpc_client.pose0")]
    device: String,

    /// Connect using only TCP
    #[structopt(short, long)]
    tcp: bool,
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    env_logger::init();

    let opts = VrpnClientCommand::from_args();
    let protocol_str = if opts.tcp { "tcp://" } else { "" };
    let hostname = format!("{}{}:{}", protocol_str, opts.server, opts.port);

    log::info!("Start VRPN client");
    analog_listen(&opts.device, &hostname);
    log::info!("VRPN client done");

    Ok(())
}
