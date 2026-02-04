use std::convert::TryInto;
use cxx::UniquePtr;

use crate::errors::MissingField;
use crate::grpc::proto::Pose3D;

#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("posenet-vr-hub/include/vrpn_forward.hpp");
        include!("posenet-vr-hub/include/vrpn.hpp");

        type PoseNetVrpnContainer;

        // Server
        // fn create_server(device_name: &str) -> UniquePtr<PoseNetVrpnContainer>;
        fn create_container() -> UniquePtr<PoseNetVrpnContainer>;
        fn update_values(
            server: &mut UniquePtr<PoseNetVrpnContainer>,
            device_name: &str,
            values: &[f64],
        );
        fn mainloop(server: &mut UniquePtr<PoseNetVrpnContainer>);

        // Client
        fn run_analog_client(connection_string: &str);
        fn run_tracker_client(connection_string: &str);
    }
}

pub fn update_values(
    server: &mut UniquePtr<ffi::PoseNetVrpnContainer>,
    device_name: &str,
    pose: Pose3D,
) -> Result<(), MissingField> {
    let values: Vec<f64> = pose.try_into()?;
    ffi::update_values(server, device_name, &values);
    Ok(())
}
