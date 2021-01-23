#pragma once
#include <math.h>
#include <stdio.h>
#include <unistd.h>

#include <iostream>
#include <memory>
#include <random>

#include "posenet-vr-hub/src/vrpn/vrpn.rs.h"
#include "vrpn_Analog.h"
#include "vrpn_Button.h"
#include "vrpn_Connection.h"
#include "vrpn_Text.h"
#include "vrpn_Tracker.h"

using namespace std;

// Triangulation

/*
unique_ptr<Vec4> triangulate_nview(
    // x's are landmark bearing vectors in each camera
    const unique_ptr<Mat3X> x,
    // Ps are projective cameras
    const unique_ptr<vector<Mat34>> Ps);
*/

void run_vrpn();

void run_analog_client(rust::Str connection_string);
void run_tracker_client(rust::Str connection_string);