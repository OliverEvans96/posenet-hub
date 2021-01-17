#pragma once

#include <iostream>
#include <memory>
#include <openMVG/numeric/eigen_alias_definition.hpp>
#include <sstream>
#include <string>

#include "posenet-vr-hub/src/lib.rs.h"

using namespace openMVG;
using namespace std;

template <typename T>
unique_ptr<string> format_mat(const T &A);
unique_ptr<string> format_mat3x(const Mat3X &A);
unique_ptr<string> format_mat34(const Mat34 &A);

unique_ptr<Mat3X> mat3x_from_data(rust::Slice<double> slice, size_t rows,
                                  size_t cols);
unique_ptr<Mat34> mat34_from_data(rust::Slice<double> slice, size_t rows,
                                  size_t cols);