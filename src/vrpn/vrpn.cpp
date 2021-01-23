#include "posenet-vr-hub/include/vrpn.hpp"

#include <vrpn_Connection.h>

using namespace std;

/***************** PoseNetVRPNServer ******************/
// Based on http://www.vrgeeks.org/vrpn/tutorial---vrpn-server

PoseNetVrpnServer::PoseNetVrpnServer(const char *device_name,
                                     shared_ptr<vrpn_Connection_IP> connection)
    : vrpn_Analog(device_name, connection.get()) {
    vrpn_Analog::num_channel = NUM_CHANNELS;

    initialize_channels();
}

PoseNetVrpnServer::~PoseNetVrpnServer() { printf("VRPN server terminating\n"); }

void PoseNetVrpnServer::initialize_channels() {
    for (vrpn_int32 i = 0; i < vrpn_Analog::num_channel; i++) {
        vrpn_Analog::channel[i] = vrpn_Analog::last[i] = 0.0;
    }
}

void PoseNetVrpnServer::update_values(rust::Slice<const double> values) {
    size_t num_values = values.length();
    if (num_values != NUM_CHANNELS) {
        cout << "ERROR: Expected " << NUM_CHANNELS << " values, but got "
             << num_values << endl;
    } else {
        for (size_t i = 0; i < num_values; i++) {
            channel[i] = values[i];
        }
    }
}

void PoseNetVrpnServer::mainloop() {
    // Update clock
    vrpn_gettimeofday(&_timestamp, NULL);
    vrpn_Analog::timestamp = _timestamp;

    // Send any changes out over the connection.
    vrpn_Analog::report_changes();

    // Do other important VRPN stuff under the hood
    server_mainloop();
}

/**************** Rust FFI Functions *****************/

unique_ptr<PoseNetVrpnContainer> create_server(rust::Str device_name) {
    string device_name_str(device_name.data(), device_name.size());
    auto connection = make_shared<vrpn_Connection_IP>();
    auto server =
        make_unique<PoseNetVrpnServer>(device_name_str.data(), connection);
    auto container =
        make_unique<PoseNetVrpnContainer>(move(server), move(connection));
    printf("VRPN server with device '%s' started.\n", device_name_str.data());
    return container;
}

void mainloop(unique_ptr<PoseNetVrpnContainer> &container,
              rust::Slice<const double> values) {
    // Update Server
    container->server->update_values(values);
    container->server->mainloop();

    // Update Connection
    container->connection->mainloop();
}

// Client
// From http://www.vrgeeks.org/vrpn/tutorial---use-vrpn

void VRPN_CALLBACK handle_analog(void *userData, const vrpn_ANALOGCB a) {
    cout << "Analog : ";

    for (int i = 0; i < a.num_channel; i++) {
        cout << a.channel[i] << " ";
    }

    cout << endl;
}

void run_analog_client(rust::Str connection_string) {
    vrpn_Analog_Remote *vrpnAnalog =
        new vrpn_Analog_Remote(connection_string.data());

    vrpnAnalog->register_change_handler(0, handle_analog);

    while (1) {
        vrpnAnalog->mainloop();
    }
}