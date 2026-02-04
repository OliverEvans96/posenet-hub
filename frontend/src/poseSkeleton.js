/**
 * Skeleton topology for 17 keypoints: pairs of keypoint indices that form bones.
 * Order matches KEYPOINT_NAMES: 0=nose, 1=left_eye, ..., 16=right_ankle.
 */
export const SKELETON_EDGES = [
  [0, 1],   // nose -> left_eye
  [0, 2],   // nose -> right_eye
  [1, 3],   // left_eye -> left_ear
  [2, 4],   // right_eye -> right_ear
  [5, 6],   // left_shoulder -> right_shoulder
  [5, 7],   // left_shoulder -> left_elbow
  [7, 9],   // left_elbow -> left_wrist
  [6, 8],   // right_shoulder -> right_elbow
  [8, 10],  // right_elbow -> right_wrist
  [5, 11],  // left_shoulder -> left_hip
  [6, 12],  // right_shoulder -> right_hip
  [11, 12], // left_hip -> right_hip
  [11, 13], // left_hip -> left_knee
  [13, 15], // left_knee -> left_ankle
  [12, 14], // right_hip -> right_knee
  [14, 16], // right_knee -> right_ankle
  [0, 5],   // nose -> left_shoulder (torso)
  [0, 6],   // nose -> right_shoulder
];

/**
 * @param {( { x: number, y: number, z: number } | null)[]} keypoints
 * @returns {[number, number][]} Array of [i, j] indices for which both keypoints are present
 */
export function getVisibleEdges(keypoints) {
  const edges = [];
  for (const [a, b] of SKELETON_EDGES) {
    const pa = keypoints[a];
    const pb = keypoints[b];
    if (pa && pb) edges.push([a, b]);
  }
  return edges;
}
