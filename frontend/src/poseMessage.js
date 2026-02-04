/**
 * Parses and validates WebSocket pose stream messages from the hub.
 * Message format: { group_name, poses: [{ keypoints: [Point3D|null], score }], timestamp_ms }
 */

const KEYPOINT_NAMES = [
  'nose',
  'left_eye',
  'right_eye',
  'left_ear',
  'right_ear',
  'left_shoulder',
  'right_shoulder',
  'left_elbow',
  'right_elbow',
  'left_wrist',
  'right_wrist',
  'left_hip',
  'right_hip',
  'left_knee',
  'right_knee',
  'left_ankle',
  'right_ankle',
];

/**
 * @typedef {{ x: number, y: number, z: number, score: number }} Point3D
 * @typedef {{ keypoints: (Point3D|null)[], score: number }} Pose3DJson
 * @typedef {{ group_name: string, poses: Pose3DJson[], timestamp_ms: number }} PoseStreamMessage
 */

/**
 * Parse JSON string into a pose stream message. Returns null if invalid.
 * @param {string} raw
 * @returns {PoseStreamMessage|null}
 */
export function parsePoseStreamMessage(raw) {
  if (typeof raw !== 'string' || !raw.trim()) return null;
  try {
    const data = JSON.parse(raw);
    if (!data || typeof data.group_name !== 'string') return null;
    if (!Array.isArray(data.poses)) return null;
    if (typeof data.timestamp_ms !== 'number' && typeof data.timestamp_ms !== 'undefined') return null;
    const poses = data.poses.map(normalizePose);
    return {
      group_name: data.group_name,
      poses,
      timestamp_ms: typeof data.timestamp_ms === 'number' ? data.timestamp_ms : 0,
    };
  } catch {
    return null;
  }
}

/**
 * Normalize a single pose so keypoints is a 17-element array (null for missing).
 * @param {unknown} p
 * @returns {Pose3DJson}
 */
export function normalizePose(p) {
  if (!p || typeof p !== 'object') return emptyPose();
  const keypoints = Array.isArray(p.keypoints) ? p.keypoints : [];
  const arr = [];
  for (let i = 0; i < 17; i++) {
    const k = keypoints[i];
    arr.push(isValidPoint(k) ? { x: k.x, y: k.y, z: k.z, score: k.score } : null);
  }
  return {
    keypoints: arr,
    score: typeof p.score === 'number' ? p.score : 0,
  };
}

function isValidPoint(k) {
  return (
    k != null &&
    typeof k === 'object' &&
    typeof k.x === 'number' &&
    typeof k.y === 'number' &&
    typeof k.z === 'number' &&
    typeof k.score === 'number'
  );
}

function emptyPose() {
  return {
    keypoints: Array(17).fill(null),
    score: 0,
  };
}

export { KEYPOINT_NAMES };
