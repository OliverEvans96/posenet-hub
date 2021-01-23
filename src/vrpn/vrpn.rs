// use cxx::{CxxVector, UniquePtr, UniquePtrTarget};

#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("posenet-vr-hub/include/vrpn.hpp");
        fn run_vrpn();
    }
}
