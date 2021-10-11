#include "posenet-vr-hub/include/openmvg.hpp"

// Triangulation

unique_ptr<Vec4> triangulate_nview(
    // 3 x N matrix, where N is the number of cameras.
    // Column j is the homogeneous 2D point observed from camera j
    const unique_ptr<Mat3X> xh,
    // Ps[j] is the 3x4 camera matrix for camera j
    const unique_ptr<vector<Mat34>> Ps) {
    auto X = make_unique<Vec4>();
    TriangulateNView(*xh, *Ps, X.get());
    return X;
}

tuple<IndexT, IndexT> get_nviews_and_npoints(
    // N-vector 2 x M matrix, where N is the number of cameras, M is the number of points.
    // Column j of xs[i] is the 2D (non-homogeneous) observation of point j from camera i
    const unique_ptr<vector<Mat2X>> &xs) {
    // Length of vectors
    IndexT nviews = xs->size();
    // Number of columns in observations matrices
    IndexT npoints = 0;

    if (nviews > 0) {
        npoints = (*xs)[0].cols();
    }

    return make_tuple(nviews, npoints);
}

bool validate_sfm_inputs(
    // N-vector 2 x M matrix, where N is the number of cameras, M is the number of points.
    // Column j of xs[i] is the 2D (non-homogeneous) observation of point j from camera i
    const unique_ptr<vector<Mat2X>> &xs,
    // Intrinsics for each camera
    const unique_ptr<vector<Mat3>> &Ks,
    // Position part of camera matrix for each camera
    const unique_ptr<vector<Vec3>> &ts,
    // Rotation part of camera matrix for each camera
    const unique_ptr<vector<Mat3>> &Rs,
    // Column j of X is the 3d reconstruction of point j
    // Also used as an initial guess for bundle adjustment
    const unique_ptr<Mat3X> &X) {

    auto [nviews, npoints] = get_nviews_and_npoints(xs);

    if (Ks->size() < nviews) {
        cout << "Ks is too small!" << endl;
        return false;
    }
    if (ts->size() < nviews) {
        cout << "ts is too small!" << endl;
        return false;
    }
    if (Rs->size() < nviews) {
        cout << "Rs is too small!" << endl;
        return false;
    }

    if (X->cols() < npoints) {
        cout << "X has too few columns" << endl;
        return false;
    }

    return true;
}

tuple<bool, sfm::SfM_Data> construct_sfm_data(
    // N-vector 2 x M matrix, where N is the number of cameras, M is the number of points.
    // Column j of xs[i] is the 2D (non-homogeneous) observation of point j from camera i
    const unique_ptr<vector<Mat2X>> &xs,
    // Intrinsics for each camera
    const unique_ptr<vector<Mat3>> &Ks,
    // Position part of camera matrix for each camera
    const unique_ptr<vector<Vec3>> &ts,
    // Rotation part of camera matrix for each camera
    const unique_ptr<vector<Mat3>> &Rs,
    // Column j of X is the 3d reconstruction of point j
    // Also used as an initial guess for bundle adjustment
    const unique_ptr<Mat3X> &X) {

    sfm::SfM_Data sfm_data;
    auto [nviews, npoints] = get_nviews_and_npoints(xs);

    for (IndexT j = 0; j < npoints; j++) {
        auto landmark = sfm::Landmark();
        // Initialize 3d point with the given guess
        landmark.X = X->col(j);
        sfm_data.structure[j] = landmark;
    }

    for (IndexT i = 0; i < nviews; i++) {
        auto x = xs->at(i);
        auto t = ts->at(i);
        auto R = Rs->at(i);
        auto K = Ks->at(i);

        if (x.cols() < npoints) {
            cout << "x[" << i << "] has too few columns" << endl;
            return make_tuple(false, sfm_data);
        }

        // Record camera pose for this view
        // NOTE: A pose here just means the camera's position and orientation
        // Get camera center from rotation & translation parts of camera matrix
        auto c = -R * t;
        sfm_data.poses[i] = geometry::Pose3(R, c);

        // Record camera i's intrinsic parameters
        // TODO: Are these necessary? Get real image dimensions from args
        unsigned int w = 681;
        unsigned int h = 481;
        // TODO: Switch to Pinhole_Intrinsic_Brown_T2,
        // which matches OpenCV's calibration params
        sfm_data.intrinsics[i] = make_shared<cameras::Pinhole_Intrinsic>(w, h, K);

        // The view object just relates the pose to the intrinsic parameters
        auto view = make_shared<sfm::View>();
        view->id_view = i;
        view->id_intrinsic = i;
        view->id_pose = i;

        // Record observation of point k from view i
        for (IndexT j = 0; j < npoints; j++) {
            auto obs = sfm::Observation();
            obs.id_feat = j;
            obs.x = x.col(j);
            sfm_data.structure[j].obs[i] = obs;
        }

        sfm_data.views[i] = view;
    }

    return make_tuple(true, sfm_data);
}

bool perform_bundle_adjustment(sfm::SfM_Data &sfm_data) {
    // Perform bundle adjustment
    const bool bVerbose = true;
    const bool bMultithread = false;
    auto ceres_opts = sfm::Bundle_Adjustment_Ceres::BA_Ceres_options(bVerbose, bMultithread);
    auto ba_object = make_shared<sfm::Bundle_Adjustment_Ceres>(ceres_opts);
    auto optimize_opts = sfm::Optimize_Options(
        cameras::Intrinsic_Parameter_Type::ADJUST_ALL,
        sfm::Extrinsic_Parameter_Type::ADJUST_ALL,
        sfm::Structure_Parameter_Type::ADJUST_ALL);
    bool result = ba_object->Adjust(sfm_data, optimize_opts);
    return result;
}
void update_sfm_inputs(
    sfm::SfM_Data &sfm_data,
    // N-vector 2 x M matrix, where N is the number of cameras, M is the number of points.
    // Column j of xs[i] is the 2D (non-homogeneous) observation of point j from camera i
    const unique_ptr<vector<Mat2X>> &xs,
    // Intrinsics for each camera
    const unique_ptr<vector<Mat3>> &Ks,
    // Position part of camera matrix for each camera
    const unique_ptr<vector<Vec3>> &ts,
    // Rotation part of camera matrix for each camera
    const unique_ptr<vector<Mat3>> &Rs,
    // Column j of X is the 3d reconstruction of point j
    // Also used as an initial guess for bundle adjustment
    const unique_ptr<Mat3X> &X) {

    auto [nviews, npoints] = get_nviews_and_npoints(xs);

    for (IndexT j = 0; j < npoints; j++) {
        // Update reconstucted 3d points
        X->col(j) = sfm_data.GetLandmarks().at(j).X;
    }

    for (IndexT i = 0; i < nviews; i++) {
        // Update camera intrinsics
        auto intrinsic = dynamic_pointer_cast<cameras::Pinhole_Intrinsic>(sfm_data.GetIntrinsics().at(i));
        Ks->at(i) = intrinsic->K();

        // Update camera extrinsics
        auto pose = sfm_data.GetPoses().at(i);
        ts->at(i) = pose.translation();
        Rs->at(i) = pose.rotation();
    }
}

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
    // Column j of X is the 3d reconstruction of point j
    // Also used as an initial guess for bundle adjustment
    unique_ptr<Mat3X> &X) {

    bool inputs_valid = validate_sfm_inputs(xs, Ks, ts, Rs, X);
    if (!inputs_valid) {
        return false;
    }

    auto [success, sfm_data] = construct_sfm_data(xs, Ks, ts, Rs, X);
    bool result = perform_bundle_adjustment(sfm_data);
    update_sfm_inputs(sfm_data, xs, Ks, ts, Rs, X);

    return result;
}
