#pragma once
#include <memory>
#include <openMVG/multiview/triangulation_nview.hpp>

#include "posenet-vr-hub/src/openmvg/openmvg.rs.h"

using namespace openMVG;
using namespace std;

// Triangulation

unique_ptr<Vec4> triangulate_nview(
    // x's are landmark bearing vectors in each camera
    const unique_ptr<Mat3X> x,
    // Ps are projective cameras
    const unique_ptr<std::vector<Mat34>> Ps);