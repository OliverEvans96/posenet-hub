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

/// https://github.com/openMVG/openMVG/blob/5e98d504bb76ba2d1d07ae80ac2acb10b3d6f97d/src/openMVG/sfm/sfm_data_BA_test.cpp#L309-L330
/// Compute the Root Mean Square Error of the residuals
double RMSE(const sfm::SfM_Data & sfm_data)
{
  // Compute residuals for each observation
  std::vector<double> vec;
  for (const auto& landmark_it : sfm_data.GetLandmarks())
  {
    const sfm::Observations & obs = landmark_it.second.obs;
    for (const auto& obs_it : obs)
    {
      const sfm::View * view = sfm_data.GetViews().find(obs_it.first)->second.get();
      const geometry::Pose3 pose = sfm_data.GetPoseOrDie(view);
      const std::shared_ptr<cameras::IntrinsicBase> intrinsic = sfm_data.GetIntrinsics().find(view->id_intrinsic)->second;
    //   printf("View %d, feat %d\n", view->id_view, obs_it.second.id_feat);
    //   cout << "3d: " << landmark_it.second.X << endl;
    //   cout << "reprojected: " << pose(landmark_it.second.X) << endl;
      const Vec2 residual = intrinsic->residual(pose(landmark_it.second.X), obs_it.second.x);
      vec.push_back( residual(0) );
      vec.push_back( residual(1) );
    }
  }
  const Eigen::Map<Eigen::RowVectorXd> residuals(&vec[0], vec.size());
//   cout << "residuals:" << endl << residuals << endl;
//   cout << "norm:" << residuals.squaredNorm() << endl;
  const double RMSE = std::sqrt(residuals.squaredNorm() / vec.size());
//   cout << "RMSE:" << RMSE << endl;
  return RMSE;
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
        auto c = -R.transpose() * t;
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

sfm::Optimize_Options construct_optimize_options(BundleAdjustmentOptions opts) {
    // Intrinsics
    auto intrinsics_type = opts.camera_intrinsics ? cameras::Intrinsic_Parameter_Type::ADJUST_ALL
                                                  : cameras::Intrinsic_Parameter_Type::NONE;

    // Extrinsics
    sfm::Extrinsic_Parameter_Type extrinsics_type;
    if (opts.camera_rotation && opts.camera_translation) {
        extrinsics_type = sfm::Extrinsic_Parameter_Type::ADJUST_ALL;
    } else if (opts.camera_translation) {
        extrinsics_type = sfm::Extrinsic_Parameter_Type::ADJUST_TRANSLATION;
    } else if (opts.camera_rotation) {
        extrinsics_type = sfm::Extrinsic_Parameter_Type::ADJUST_ROTATION;
    } else {
        extrinsics_type = sfm::Extrinsic_Parameter_Type::NONE;
    }

    // Structure
    auto structure_type = opts.pose3d ? sfm::Structure_Parameter_Type::ADJUST_ALL : sfm::Structure_Parameter_Type::NONE;

    cout << "intrinsics_type: " << (int) intrinsics_type << endl;
    cout << "extrinsics_type: " << (int) extrinsics_type << endl;
    cout << "structure_type: " << (int) structure_type << endl;

    // Combine
    auto optimize_opts = sfm::Optimize_Options(intrinsics_type, extrinsics_type, structure_type);

    return optimize_opts;
}

bool perform_bundle_adjustment(sfm::SfM_Data &sfm_data, sfm::Optimize_Options optimize_opts) {
    // Perform bundle adjustment
    const bool bVerbose = true;
    const bool bMultithread = false;
    auto ceres_opts = sfm::Bundle_Adjustment_Ceres::BA_Ceres_options(bVerbose, bMultithread);
    auto ba_object = make_shared<sfm::Bundle_Adjustment_Ceres>(ceres_opts);
    int i = 0;
    for (const auto& pose_it : sfm_data.GetPoses()) {
        auto pose = pose_it.second;
        cout << "pose " << i << " center before:" << endl;
        cout << pose.center() << endl;
    }
    
    bool result = ba_object->Adjust(sfm_data, optimize_opts);

    for (const auto& pose_it : sfm_data.GetPoses()) {
        auto pose = pose_it.second;
        cout << "pose " << i++ << " center after:" << endl;
        cout << pose.center() << endl;
    }
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

        cout << "ts[" << i << "] before:" << endl;
        cout << ts->at(i) << endl;
        cout << "Rs[" << i << "] after:" << endl;
        cout << Rs->at(i) << endl;

        // Update camera extrinsics
        auto pose = sfm_data.GetPoses().at(i);
        ts->at(i) = pose.translation();
        Rs->at(i) = pose.rotation();

        cout << "Rs[" << i << "] after:" << endl;
        cout << ts->at(i) << endl;
        cout << "ts[" << i << "] after:" << endl;
        cout << Rs->at(i) << endl;
        cout << endl;
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
    unique_ptr<Mat3X> &X,
    BundleAdjustmentOptions opts) {

    bool inputs_valid = validate_sfm_inputs(xs, Ks, ts, Rs, X);
    if (!inputs_valid) {
        return false;
    }

    auto [success, sfm_data] = construct_sfm_data(xs, Ks, ts, Rs, X);
    auto optimize_opts = construct_optimize_options(opts);
    // auto rb = RMSE(sfm_data);
    // cout << "residual before = " << rb << endl;
    bool result = perform_bundle_adjustment(sfm_data, optimize_opts);
    // auto ra = RMSE(sfm_data);
    // cout << "residual after = " << ra << endl;
    update_sfm_inputs(sfm_data, xs, Ks, ts, Rs, X);

    return result;
}
