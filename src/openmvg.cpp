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
unique_ptr<string> format_vec3(const Vec3 &A) { return format_mat(A); }
unique_ptr<string> format_vec4(const Vec4 &A) { return format_mat(A); }

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

void print_mat34_vec(unique_ptr<vector<Mat34>> vp) {
    cout << "Mat34 vec has " << vp->size() << " elements" << endl;
    int i = 0;
    for (vector<Mat34>::iterator it = vp->begin(); it != vp->end(); ++it) {
        cout << "i = " << i << endl;
        cout << *it << endl << endl;
    }
}

// Triangulation

/*
void triangulate_nview() {
    for (int i = 0; i < npoints; ++i) {
        // Collect the image of point i in each frame.
        Mat3X xs(3, nviews);
        for (int j = 0; j < nviews; ++j) {
            xs.col(j) = d._x[j].col(i).homogeneous();
        }
        Vec4 X;
        TriangulateNView(xs, Ps, &X);

        // Check reprojection error. Should be nearly zero.
        for (int j = 0; j < nviews; ++j) {
            const Vec3 x_reprojected = Ps[j] * X;
            const double error =
                (x_reprojected.hnormalized() - xs.col(j).hnormalized()).norm();
            EXPECT_NEAR(error, 0.0, 1e-9);
        }
    }
}
}
*/

unique_ptr<Vec4> triangulate_nview(
    // x's are landmark bearing vectors in each camera
    const unique_ptr<Mat3X> x,
    // Ps are projective cameras
    const unique_ptr<std::vector<Mat34>> Ps) {
    auto X = make_unique<Vec4>();
    TriangulateNView(*x, *Ps, X.get());
    return X;
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