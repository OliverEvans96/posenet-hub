#include "posenet-vr-hub/include/openmvg.hpp"

// Triangulation

unique_ptr<Vec4> triangulate_nview(
    // 3 x N matrix, where N is the number of cameras.
    // Column j is the homogeneous 2D point observed from camera j
    const unique_ptr<Mat3X> xh,
    // Ps[j] is the 3x4 camera matrix for camera j
    const unique_ptr<vector<Mat34>> Ps)
{
    auto X = make_unique<Vec4>();
    TriangulateNView(*xh, *Ps, X.get());
    return X;
}

// void linf_bundle_adjustment(
//     // 2 x N matrix, where N is the number of cameras.
//     // Column j is the 2D (non-homogeneous) point observed from camera j
//     const unique_ptr<Mat2X> x,
//     // Ps[j] is the 3x4 camera matrix for camera j
//     const unique_ptr<vector<Mat34>> Ps
// ) {
//     // Based on https://github.com/openMVG/openMVG/blob/master/src/openMVG/linearProgramming/lInfinityCV/triangulation_test.cpp

//     // TODO

//     std::vector<double> vec_solution(3);

//     OSI_CLP_SolverWrapper wrapperOSICLPSolver(3);  // 3 parameters (x, y, z)
//     // sourcee: https://github.com/openMVG/openMVG/blob/master/src/openMVG/linearProgramming/lInfinityCV/triangulation.cpp
//     Triangulation_L1_ConstraintBuilder cstBuilder(vec_Pi, x_ij);
//     // Use bisection in order to find the global optimum and so find the
//     //  best triangulated point under the L_infinity norm
//     // source: https://github.com/openMVG/openMVG/blob/master/src/openMVG/linearProgramming/bisectionLP.hpp
//     BisectionLP<Triangulation_L1_ConstraintBuilder,LP_Constraints>(
//     wrapperOSICLPSolver,
//     cstBuilder,
//     &vec_solution,
//     1.0, // gammaUp
//     0.0 // gammaLow
//     );
// }

// From https://github.com/openMVG/openMVG/blob/5e98d504bb76ba2d1d07ae80ac2acb10b3d6f97d/src/openMVG/sfm/sfm_data_BA_test.cpp#L309-L330
// /// Compute the Root Mean Square Error of the residuals
// double RMSE(const sfm::SfM_Data & sfm_data)
// {
//   // Compute residuals for each observation
//   std::vector<double> vec;
//   for (const auto& landmark_it : sfm_data.GetLandmarks())
//   {
//     const sfm::Observations & obs = landmark_it.second.obs;
//     for (const auto& obs_it : obs)
//     {
//       const sfm::View * view = sfm_data.GetViews().find(obs_it.first)->second.get();
//       const geometry::Pose3 pose = sfm_data.GetPoseOrDie(view);
//       const std::shared_ptr<cameras::IntrinsicBase> intrinsic = sfm_data.GetIntrinsics().find(view->id_intrinsic)->second;
//       const Vec2 residual = intrinsic->residual(pose(landmark_it.second.X), obs_it.second.x);
//       vec.push_back( residual(0) );
//       vec.push_back( residual(1) );
//     }
//   }
//   const Eigen::Map<Eigen::RowVectorXd> residuals(&vec[0], vec.size());
//   cout << "Residuals: " << residuals << endl;
//   const double RMSE = std::sqrt(residuals.squaredNorm() / vec.size());
//   return RMSE;
// }

bool ceres_bundle_adjustment(
    // N-vector 2 x M matrix, where N is the number of cameras, M is the number of points.
    // Column j of xs[i] is the 2D (non-homogeneous) observation of point j from camera i
    const unique_ptr<vector<Mat2X>> xs,
    // Intrinsics for each camera
    const unique_ptr<vector<Mat3>> Ks,
    // Position part of camera matrix for each camera
    const unique_ptr<vector<Vec3>> ts,
    // Rotation part of camera matrix for each camera
    const unique_ptr<vector<Mat3>> Rs,
    // Column j of X is the 3d reconstruction of point j
    // Also used as an initial guess for bundle adjustment
    const unique_ptr<Mat3X> X)
{
    sfm::SfM_Data sfm_data;

    // Length of vectors
    IndexT nviews = xs->size();
    // Number of columns in observations matrices
    IndexT npoints = 0;

    if (nviews > 0)
    {
        npoints = (*xs)[0].cols();
    }

    if (Ks->size() < nviews)
    {
        cout << "Ks is too small!" << endl;
        return false;
    }
    if (ts->size() < nviews)
    {
        cout << "ts is too small!" << endl;
        return false;
    }
    if (Rs->size() < nviews)
    {
        cout << "Rs is too small!" << endl;
        return false;
    }

    if (X->cols() < npoints)
    {
        cout << "X has too few columns" << endl;
    }

    for (IndexT j = 0; j < npoints; j++)
    {
        auto landmark = sfm::Landmark();
        // Initialize 3d point with the given guess
        landmark.X = X->col(j);
        sfm_data.structure[j] = landmark;
    }

    for (IndexT i = 0; i < nviews; i++)
    {
        auto x = (*xs)[i];
        auto t = (*ts)[i];
        auto R = (*Rs)[i];
        auto K = (*Ks)[i];

        if (x.cols() < npoints)
        {
            cout << "x[" << i << "] has too few columns" << endl;
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
        // TODO: Is this the correct camera model?
        sfm_data.intrinsics[i] = make_shared<cameras::Pinhole_Intrinsic>(w, h, K);

        // The view object just relates the pose to the intrinsic parameters
        auto view = make_shared<sfm::View>();
        view->id_view = i;
        view->id_intrinsic = i;
        view->id_pose = i;

        // Record observation of point k from view i
        for (IndexT j = 0; j < npoints; j++)
        {
            auto obs = sfm::Observation();
            obs.id_feat = j;
            obs.x = x.col(j);
            sfm_data.structure[j].obs[i] = obs;
        }

        sfm_data.views[i] = view;
    }

    // const double dResidual_before = RMSE(sfm_data);
    // cout << "Residual before = " << dResidual_before << endl;

    // Perform bundle adjustment
    const bool bVerbose = true;
    const bool bMultithread = false;
    auto ceres_opts = sfm::Bundle_Adjustment_Ceres::BA_Ceres_options(bVerbose, bMultithread);
    auto ba_object =
        make_shared<sfm::Bundle_Adjustment_Ceres>(ceres_opts);
    auto optimize_opts =
        sfm::Optimize_Options(
            cameras::Intrinsic_Parameter_Type::ADJUST_ALL,
            sfm::Extrinsic_Parameter_Type::ADJUST_ALL,
            sfm::Structure_Parameter_Type::ADJUST_ALL);
    bool result = ba_object->Adjust(sfm_data, optimize_opts);

    // const double dResidual_after = RMSE(sfm_data);
    // cout << "Residual after = " << dResidual_after << endl;

    for (IndexT j = 0; j < npoints; j++)
    {
        cout << "j=" << j << endl;
        cout << sfm_data.GetLandmarks().at(j).X << endl
             << endl;
    }

    // Update input arguments with optimized values

    return result;
}
