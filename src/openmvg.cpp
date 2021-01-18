#include "posenet-vr-hub/include/openmvg.hpp"

// Matrix basics

template <typename T>
unique_ptr<string> format_mat(const T &A) {
    stringstream ss;
    ss << A;
    auto s = make_unique<string>(ss.str());
    return s;
}
unique_ptr<string> format_mat2x(const Mat2X &A) { return format_mat(A); }
unique_ptr<string> format_mat3x(const Mat3X &A) { return format_mat(A); }
unique_ptr<string> format_mat34(const Mat34 &A) { return format_mat(A); }
unique_ptr<string> format_vec4(const Vec4 &A) { return format_mat(A); }

unique_ptr<Mat2X> mat2x_from_data(rust::Slice<double> slice, size_t rows,
                                  size_t cols) {
    Map<Mat2X> mf(slice.data(), rows, cols);
    return make_unique<Mat2X>(mf);
}

unique_ptr<Mat3X> mat3x_from_data(rust::Slice<double> slice, size_t rows,
                                  size_t cols) {
    Map<Mat3X> mf(slice.data(), rows, cols);
    return make_unique<Mat3X>(mf);
}

unique_ptr<Mat34> mat34_from_data(rust::Slice<double> slice, size_t rows,
                                  size_t cols) {
    Map<Mat34> mf(slice.data(), rows, cols);
    return make_unique<Mat34>(mf);
}

// Triangulation

void triangulate_nview(
    const Mat3X &x,  // x's are landmark bearing vectors in each camera
    const std::vector<Mat34> &Ps,  // Ps are projective cameras
    unique_ptr<Vec4> X) {
    TriangulateNView(x, Ps, X.get());
}

// Test

NViewPartialDataset create_nview_dataset(int nviews, int npoints) {
    const NViewDataSet d = NRealisticCamerasRing(nviews, npoints);
    auto Ps = make_unique<vector<Mat34>>();
    for (size_t i = 0; i < d._n; i++) {
        Ps->push_back(d.P(i));
    }
    auto x3d = make_unique<Mat3X>(d._X);
    auto x2d_vec = make_unique<vector<Mat2X>>(d._x);
    NViewPartialDataset n{.x3d = move(x3d),
                          .x2d_vec = move(x2d_vec),
                          .n = d._n,
                          .p_vec = move(Ps)};
    return n;
}