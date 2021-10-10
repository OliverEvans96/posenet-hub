#pragma once
#include <vrpn_Analog.h>
#include <vrpn_Connection.h>

#include <iostream>
#include <map>
#include <memory>
#include <random>

#include "posenet-vr-hub/src/vrpn/vrpn.rs.h"

/********************* Constants **********************/

// PoseNet returns 17 points on the body
const uint32_t NUM_KEYPOINTS = 17;
// (x,y,z,score) for 17 keypoints + pose score
const uint32_t NUM_CHANNELS = 4 * NUM_KEYPOINTS + 1;

/***************** PoseNetVRPNServer ******************/
class PoseNetVrpnServer : public vrpn_Analog {
  public:
    PoseNetVrpnServer(const char *device_name, std::shared_ptr<vrpn_Connection_IP> connection);
    ~PoseNetVrpnServer();

    void update_values(rust::Slice<const double> values);
    virtual void mainloop();

  protected:
    struct timeval _timestamp;

    void initialize_channels();
};

class PoseNetVrpnContainer {
  public:
    PoseNetVrpnContainer( // std::unique_ptr<PoseNetVrpnServer> _server,
        std::shared_ptr<vrpn_Connection_IP> _connection) {
        // server = move(_server);
        connection = move(_connection);
    };
    // std::unique_ptr<PoseNetVrpnServer> server;
    std::map<std::string, std::unique_ptr<PoseNetVrpnServer>> servers;
    std::shared_ptr<vrpn_Connection_IP> connection;
};

/**************** Rust FFI Functions *****************/

// Server
std::unique_ptr<PoseNetVrpnContainer> create_container();
// std::unique_ptr<PoseNetVrpnContainer> create_server(rust::Str device_name);

void update_values(
    std::unique_ptr<PoseNetVrpnContainer> &container, rust::Str device_name, rust::Slice<const double> values);

void mainloop(std::unique_ptr<PoseNetVrpnContainer> &container);

// Client
void run_analog_client(rust::Str connection_string);
void run_tracker_client(rust::Str connection_string);