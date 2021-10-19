#pragma once
#include <iostream>
#include <memory>
#include <openMVG/multiview/triangulation_nview.hpp>

#include <openMVG/cameras/Camera_Pinhole.hpp>
#include <openMVG/geometry/pose3.hpp>
#include <openMVG/sfm/sfm_data.hpp>
#include <openMVG/sfm/sfm_data_BA_ceres.hpp>

#include "posenet-vr-hub/src/openmvg/openmvg.rs.h"

using namespace openMVG;
using namespace std;

// Triangulation

unique_ptr<Vec4> triangulate_nview(
    // 3 x N matrix, where N is the number of cameras.
    // Column j is the homogeneous 2D point point observed from camera j
    const unique_ptr<Mat3X> xh,
    // Ps[j] is the 3x4 camera matrix for camera j
    const unique_ptr<vector<Mat34>> Ps);

bool ceres_bundle_adjustment(
    // N-vector 2 x M matrix, where N is the number of cameras, M is the number of points.
    // Column j of xs[i] is the 2D (non-homogeneous) observation of point j from camera i
    const unique_ptr<vector<Mat2X>> &xs,
    // Intrinsics for each camera
    unique_ptr<vector<Mat3>> &Ks,
    // Position part of camera matrix for each camera
    unique_ptr<vector<Vec3>> &ts,
    // Rotation part of camera matrix for each camera
    unique_ptr<vector<Mat3>> &Rs,
    // Column j of X is the 3d reruction of point j
    // Also used as an initial guess for bundle adjustment
    unique_ptr<Mat3X> &X,
    BundleAdjustmentOptions opts);