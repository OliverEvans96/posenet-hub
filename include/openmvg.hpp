#pragma once

#include <iostream>
#include <memory>
#include <openMVG/multiview/test_data_sets.hpp>
#include <openMVG/multiview/triangulation_nview.hpp>
#include <sstream>
#include <string>

#include "posenet-vr-hub/src/lib.rs.h"

using namespace openMVG;
using namespace std;

// Matrix basics

template <typename T>
unique_ptr<string> format_mat(const T &A);
unique_ptr<string> format_mat2x(const Mat2X &A);
unique_ptr<string> format_mat3x(const Mat3X &A);
unique_ptr<string> format_mat34(const Mat34 &A);
unique_ptr<string> format_vec3(const Vec3 &A);
unique_ptr<string> format_vec4(const Vec4 &A);

unique_ptr<Mat2X> mat2x_from_data(rust::Slice<double> slice, size_t cols);
unique_ptr<Mat3X> mat3x_from_data(rust::Slice<double> slice, size_t cols);
unique_ptr<Mat34> mat34_from_data(rust::Slice<double> slice);

unique_ptr<vector<Mat34>> mat34_vec_from_data(
    rust::Slice<const rust::Slice<double>> slices);

void print_mat34_vec(unique_ptr<vector<Mat34>> vp);

// Test data

// Triangulation

unique_ptr<Vec4> triangulate_nview(
    // x's are landmark bearing vectors in each camera
    const unique_ptr<Mat3X> x,
    // Ps are projective cameras
    const unique_ptr<std::vector<Mat34>> Ps);

// Test

NViewPartialDataset create_nview_dataset(int nviews, int npoints);
