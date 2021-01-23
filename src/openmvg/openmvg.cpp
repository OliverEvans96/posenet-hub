#include "posenet-vr-hub/include/openmvg.hpp"

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