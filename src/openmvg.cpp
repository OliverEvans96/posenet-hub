#include "posenet-vr-hub/include/openmvg.hpp"

template <typename T>
unique_ptr<string> format_mat(const T &A) {
    stringstream ss;
    ss << A;
    auto s = make_unique<string>(ss.str());
    return s;
}
unique_ptr<string> format_mat3x(const Mat3X &A) { return format_mat(A); }
unique_ptr<string> format_mat34(const Mat34 &A) { return format_mat(A); }

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
