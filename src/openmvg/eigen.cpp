#include "posenet-vr-hub/include/eigen.hpp"

// Format

template <typename T> unique_ptr<string> format_mat(const T &A) {
    stringstream ss;
    ss << A;
    auto s = make_unique<string>(ss.str());
    return s;
}
unique_ptr<string> format_mat2x(const Mat2X &A) { return format_mat(A); }
unique_ptr<string> format_mat3x(const Mat3X &A) { return format_mat(A); }
unique_ptr<string> format_mat34(const Mat34 &A) { return format_mat(A); }
unique_ptr<string> format_mat3(const Mat3 &A) { return format_mat(A); }
unique_ptr<string> format_vec3(const Vec3 &A) { return format_mat(A); }
unique_ptr<string> format_vec4(const Vec4 &A) { return format_mat(A); }
unique_ptr<string> format_vec2(const Vec2 &A) { return format_mat(A); }

// To Eigen

unique_ptr<Mat2X> mat2x_from_data(rust::Slice<const double> slice, size_t cols) {
    const int rows = 2;
    Map<const Mat2X> mf(slice.data(), rows, cols);
    return make_unique<Mat2X>(mf);
}

unique_ptr<Mat3X> mat3x_from_data(rust::Slice<const double> slice, size_t cols) {
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

unique_ptr<vector<Mat34>> mat34_vec_from_data(const rust::Slice<const rust::Slice<const double>> slices) {
    const int rows = 3;
    const int cols = 4;
    auto vp = make_unique<vector<Mat34>>();
    for (rust::Slice<const rust::Slice<const double>>::iterator it = slices.begin(); it != slices.end(); ++it) {
        Map<const Mat34> mf(it->data(), rows, cols);
        Mat34 mat(mf);
        vp->push_back(mat);
    }
    return vp;
}

unique_ptr<vector<Mat3>> mat3_vec_from_data(const rust::Slice<const rust::Slice<const double>> slices) {
    const int rows = 3;
    const int cols = 3;
    auto vp = make_unique<vector<Mat3>>();
    for (rust::Slice<const rust::Slice<const double>>::iterator it = slices.begin(); it != slices.end(); ++it) {
        Map<const Mat3> mf(it->data(), rows, cols);
        Mat3 mat(mf);
        vp->push_back(mat);
    }
    return vp;
}

unique_ptr<vector<Mat2X>> mat2x_vec_from_data(const rust::Slice<const rust::Slice<const double>> slices, size_t cols) {
    const int rows = 2;
    auto vp = make_unique<vector<Mat2X>>();
    for (rust::Slice<const rust::Slice<const double>>::iterator it = slices.begin(); it != slices.end(); ++it) {
        Map<const Mat2X> mf(it->data(), rows, cols);
        Mat2X mat(mf);
        vp->push_back(mat);
    }
    return vp;
}

unique_ptr<vector<Mat3X>> mat3x_vec_from_data(const rust::Slice<const rust::Slice<const double>> slices, size_t cols) {
    const int rows = 3;
    auto vp = make_unique<vector<Mat3X>>();
    for (rust::Slice<const rust::Slice<const double>>::iterator it = slices.begin(); it != slices.end(); ++it) {
        Map<const Mat3X> mf(it->data(), rows, cols);
        Mat3X mat(mf);
        vp->push_back(mat);
    }
    return vp;
}

unique_ptr<vector<Vec3>> vec3_vec_from_data(const rust::Slice<const rust::Slice<const double>> slices) {
    const int rows = 3;
    auto vp = make_unique<vector<Vec3>>();
    for (rust::Slice<const rust::Slice<const double>>::iterator it = slices.begin(); it != slices.end(); ++it) {
        Map<const Vec3> mf(it->data(), rows, 1);
        Vec3 vec(mf);
        vp->push_back(vec);
    }
    return vp;
}

unique_ptr<Mat3> mat3_from_data(rust::Slice<const double> slice) {
    const int rows = 3;
    const int cols = 3;
    Map<const Mat3> mf(slice.data(), rows, cols);
    return make_unique<Mat3>(mf);
}

unique_ptr<Vec2> vec2_from_data(rust::Slice<const double> slice) {
    const int rows = 2;
    const int cols = 1;
    Map<const Vec2> mf(slice.data(), rows, cols);
    return make_unique<Vec2>(mf);
}

// To Nalgebra

template <typename T> rust::Slice<const double> mat_to_slice(const T &mat) {
    const double *data_ptr = &(mat)(0);
    rust::Slice<const double> slice{data_ptr, static_cast<std::size_t>(mat.size())};
    return slice;
}

rust::Slice<const double> mat34_to_slice(const Mat34 &mat) { return mat_to_slice(mat); }
rust::Slice<const double> mat3_to_slice(const Mat3 &mat) { return mat_to_slice(mat); }
rust::Slice<const double> mat3x_to_slice(const Mat3X &mat) { return mat_to_slice(mat); }
rust::Slice<const double> vec4_to_slice(const Vec4 &mat) { return mat_to_slice(mat); }
rust::Slice<const double> vec3_to_slice(const Vec3 &mat) { return mat_to_slice(mat); }