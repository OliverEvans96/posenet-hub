// use cxx::{CxxVector, UniquePtr, UniquePtrTarget};

#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("posenet-vr-hub/include/vrpn.hpp");
        fn run_vrpn();
        fn run_analog_client(connection_string: &str);
        fn run_tracker_client(connection_string: &str);
    }
}
