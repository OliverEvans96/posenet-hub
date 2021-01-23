#include "posenet-vr-hub/include/vrpn.hpp"
/*

// Triangulation

unique_ptr<Vec4> triangulate_nview(
    // x's are landmark bearing vectors in each camera
    const unique_ptr<Mat3X> x,
    // Ps are projective cameras
    const unique_ptr<vector<Mat34>> Ps) {
    auto X = make_unique<Vec4>();
    TriangulateNView(*x, *Ps, X.get());
    return X;
}
*/

// From http://www.vrgeeks.org/vrpn/tutorial---vrpn-server
// VRPN Server tutorial
// by Sebastien Kuntz, for the VR Geeks (http://www.vrgeeks.org)
// August 2011

/////////////////////// TRACKER /////////////////////////////

// your tracker class must inherit from the vrpn_Tracker class
class myTracker : public vrpn_Tracker {
   public:
    myTracker(vrpn_Connection *c = 0);
    virtual ~myTracker(){};

    virtual void mainloop();

   protected:
    struct timeval _timestamp;
};

myTracker::myTracker(vrpn_Connection *c /*= 0 */)
    : vrpn_Tracker("Tracker0", c) {}

void myTracker::mainloop() {
    vrpn_gettimeofday(&_timestamp, NULL);

    vrpn_Tracker::timestamp = _timestamp;

    // We will just put a fake data in the position of our tracker
    static float angle = 0;
    angle += 0.001f;

    // the pos array contains the position value of the tracker
    // XXX Set your values here
    pos[0] = sinf(angle);
    pos[1] = 0.0f;
    pos[2] = 0.0f;

    // the d_quat array contains the orientation value of the tracker, stored as
    // a quaternion
    // XXX Set your values here
    d_quat[0] = 0.0f;
    d_quat[1] = 0.0f;
    d_quat[2] = 0.0f;
    d_quat[3] = 1.0f;

    char msgbuf[1000];

    d_sensor = 0;

    int len = vrpn_Tracker::encode_to(msgbuf);

    if (d_connection->pack_message(len, _timestamp, position_m_id, d_sender_id,
                                   msgbuf, vrpn_CONNECTION_LOW_LATENCY)) {
        fprintf(stderr, "can't write message: tossing\n");
    }

    server_mainloop();
}

/*
/////////////////////// ANALOG /////////////////////////////

// your analog class must inherin from the vrpn_Analog class
class myAnalog : public vrpn_Analog {
   public:
    myAnalog(vrpn_Connection *c, float _x);
    ~myAnalog() { rust_stop(&status); };

    virtual void mainloop();

   protected:
    struct timeval _timestamp;
    float x;
    RustVrpnStatus status;
};

myAnalog::myAnalog(vrpn_Connection *c, float _x)
    : vrpn_Analog("Analog0", c) {
    x = _x;
    vrpn_Analog::num_channel = 10;

    vrpn_uint32 i;

    for (i = 0; i < (vrpn_uint32)vrpn_Analog::num_channel; i++) {
        vrpn_Analog::channel[i] = vrpn_Analog::last[i] = rust_func(x);
    }

    // Start background thread to update values from rust
    status = rust_create_status();
    rust_init(vrpn_Analog::channel, vrpn_Analog::num_channel, &status);
}

void myAnalog::mainloop() {
    vrpn_gettimeofday(&_timestamp, NULL);
    vrpn_Analog::timestamp = _timestamp;

    static int i = 0;

    // Call rust to modify channel values
    // rust_mod_arr(channel, vrpn_Analog::num_channel);

    // Send any changes out over the connection.
    vrpn_Analog::report_changes();

    server_mainloop();

    if (i++ == 5) rust_stop(&status);
}

/////////////////////// BUTTON /////////////////////////////

// your button class must inherit from the vrpn_Button class
class myButton : public vrpn_Button {
   public:
    myButton(vrpn_Connection *c = 0);
    virtual ~myButton(){};

    virtual void mainloop();

   protected:
    struct timeval _timestamp;
};

myButton::myButton(vrpn_Connection *c) : vrpn_Button("Button0", c) {
    // Setting the number of buttons to 10
    vrpn_Button::num_buttons = 10;

    vrpn_uint32 i;

    // initializing all buttons to false
    for (i = 0; i < (vrpn_uint32)vrpn_Button::num_buttons; i++) {
        vrpn_Button::buttons[i] = vrpn_Button::lastbuttons[i] = 0;
    }
}

void myButton::mainloop() {
    vrpn_gettimeofday(&_timestamp, NULL);
    vrpn_Button::timestamp = _timestamp;

    // forcing values to change otherwise vrpn doesn't report the changes
    static int b = 0;
    b++;

    for (unsigned int i = 0; i < vrpn_Button::num_buttons; i++) {
        // XXX Set your values here !
        buttons[i] = (i + b) % 2;
    }

    // Send any changes out over the connection.
    vrpn_Button::report_changes();

    server_mainloop();
}

*/
////////////// MAIN ///////////////////

void run_vrpn() {
    // Creating the network server
    vrpn_Connection_IP *m_Connection = new vrpn_Connection_IP();

    // Creating the tracker
    myTracker *serverTracker = new myTracker(m_Connection);
    // myAnalog *serverAnalog = new myAnalog(m_Connection, 4);
    // myButton *serverButton = new myButton(m_Connection);

    cout << "Created VRPN server." << endl;

    while (true) {
        serverTracker->mainloop();
        // serverAnalog->mainloop();
        // serverButton->mainloop();

        m_Connection->mainloop();

        // Calling Sleep to let the CPU breathe.
        sleep(1);
    }
}