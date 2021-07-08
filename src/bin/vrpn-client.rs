
use clap::App;

use posenet_vr_hub::vrpn::client::analog_listen;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new("vrpn-client")
                    .version("0.1.0")
                    .about("Test vrpn-client receives 3D poses from posenet hub-server.")
                    .args_from_usage(
                    "-s, --server=[server] 'Server address (default: localhost)'
                    -p, --port=[port]      'VRPN port (default: 3883)'
                    -t, --tcp              'Connect using only tcp'
                    -d, --device=[device]  'Vrpn device name (default: grpc_client.pose0)'")
                    .get_matches();

    // Get config flags or defaults
    let server = matches.value_of("server").unwrap_or("127.0.0.1");
    let port = matches.value_of("port").unwrap_or("3883");
    let device = matches.value_of("device").unwrap_or("grpc_client.pose0");
    let tcp = if matches.occurrences_of("tcp") > 0 {"tcp://"} else {""};

    let hostname = format!("{}{}:{}", tcp, server, port);

    println!("Start VRPN client");
    analog_listen(device, &hostname);
    println!("VRPN client done");

    Ok(())
}
