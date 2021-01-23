use super::vrpn::ffi;

pub fn analog_listen(device_name: &str, hostname: &str) {
    let connection_string = format!("{}@{}", device_name, hostname);
    ffi::run_analog_client(&connection_string);
}

pub fn tracker_listen(device_name: &str, hostname: &str) {
    let connection_string = format!("{}@{}", device_name, hostname);
    ffi::run_tracker_client(&connection_string);
}
