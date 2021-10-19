#pragma once
#include <memory>
#include <openMVG/numeric/eigen_alias_definition.hpp>

#include "posenet-vr-hub/src/openmvg/eigen.rs.h"

using namespace openMVG;
using namespace std;

// Format
template <typename T> unique_ptr<string> format_mat(const T &A);
unique_ptr<string> format_mat2x(const Mat2X &A);
unique_ptr<string> format_mat3x(const Mat3X &A);
unique_ptr<string> format_mat34(const Mat34 &A);
unique_ptr<string> format_mat3(const Mat3 &A);
unique_ptr<string> format_vec3(const Vec3 &A);
unique_ptr<string> format_vec4(const Vec4 &A);
unique_ptr<string> format_vec2(const Vec2 &A);

// To Eigen
unique_ptr<Mat2X> mat2x_from_data(rust::Slice<const double> slice, size_t cols);
unique_ptr<Mat3X> mat3x_from_data(rust::Slice<const double> slice, size_t cols);
unique_ptr<Mat34> mat34_from_data(rust::Slice<const double> slice);
unique_ptr<Mat3> mat3_from_data(rust::Slice<const double> slice);
unique_ptr<vector<Mat34>> mat34_vec_from_data(rust::Slice<const rust::Slice<const double>> slices);
unique_ptr<vector<Mat3>> mat3_vec_from_data(rust::Slice<const rust::Slice<const double>> slices);
unique_ptr<vector<Mat2X>> mat2x_vec_from_data(rust::Slice<const rust::Slice<const double>> slices, size_t cols);
unique_ptr<vector<Mat3X>> mat3x_vec_from_data(rust::Slice<const rust::Slice<const double>> slices, size_t cols);
unique_ptr<vector<Vec3>> vec3_vec_from_data(const rust::Slice<const rust::Slice<const double>> slices);

unique_ptr<Vec2> vec2_from_data(rust::Slice<const double> slice);

// To Nalgebra
rust::Slice<const double> mat34_to_slice(const Mat34 &mat);
rust::Slice<const double> mat3_to_slice(const Mat3 &mat);
rust::Slice<const double> mat3x_to_slice(const Mat3X &mat);
rust::Slice<const double> vec4_to_slice(const Vec4 &mat);
rust::Slice<const double> vec3_to_slice(const Vec3 &mat);
