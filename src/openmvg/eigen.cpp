#include "posenet-vr-hub/include/eigen.hpp"

// Format

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
unique_ptr<string> format_vec3(const Vec3 &A) { return format_mat(A); }
unique_ptr<string> format_vec4(const Vec4 &A) { return format_mat(A); }

// To Eigen

unique_ptr<Mat2X> mat2x_from_data(rust::Slice<const double> slice,
                                  size_t cols) {
    const int rows = 2;
    Map<const Mat2X> mf(slice.data(), rows, cols);
    return make_unique<Mat2X>(mf);
}

unique_ptr<Mat3X> mat3x_from_data(rust::Slice<const double> slice,
                                  size_t cols) {
    const int rows = 3;
    Map<const Mat3X> mf(slice.data(), rows, cols);
    return make_unique<Mat3X>(mf);
}

unique_ptr<Mat34> mat34_from_data(rust::Slice<const double> slice) {
    const int rows = 3;
    const int cols = 4;
    Map<const Mat34> mf(slice.data(), rows, cols);
    return make_unique<Mat34>(mf);
}

unique_ptr<vector<Mat34>> mat34_vec_from_data(
    const rust::Slice<const rust::Slice<const double>> slices) {
    const int rows = 3;
    const int cols = 4;
    auto vp = make_unique<vector<Mat34>>();
    for (rust::Slice<const rust::Slice<const double>>::iterator it =
             slices.begin();
         it != slices.end(); ++it) {
        Map<const Mat34> mf(it->data(), rows, cols);
        Mat34 mat(mf);
        vp->push_back(mat);
    }
    return vp;
}

// To Nalgebra

rust::Slice<const double> mat34_to_slice(const unique_ptr<Mat34> &mat_ptr) {
    // See Eigen Map docs
    // https://eigen.tuxfamily.org/dox/group__TutorialMapClass.html
    const size_t rows = 3;
    const size_t cols = 4;
    double *data_ptr = &(*mat_ptr)(0);
    Map<Mat34> mf(data_ptr, rows, cols);
    rust::Slice<const double> slice{mf.data(), (size_t)mf.size()};
    return slice;
}

rust::Slice<const double> vec4_to_slice(const unique_ptr<Vec4> &mat_ptr) {
    // See Eigen Map docs
    // https://eigen.tuxfamily.org/dox/group__TutorialMapClass.html
    const size_t rows = 4;
    double *data_ptr = &(*mat_ptr)(0);
    Map<Vec4> mf(data_ptr, rows);
    rust::Slice<const double> slice{mf.data(), (size_t)mf.size()};
    return slice;
}