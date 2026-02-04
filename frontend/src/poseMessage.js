/**
 * Parses and validates WebSocket pose stream messages from the hub.
 * Message format: { group_name, poses, camera_views?, cameras?, timestamp_ms }
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
 * @typedef {{ x: number, y: number, score: number }} Point2D
 * @typedef {{ keypoints: (Point2D|null)[], score: number }} Pose2DJson
 * @typedef {{ camera_name: string, poses: Pose2DJson[] }} CameraViewJson
 * @typedef {{ camera_name: string, fx: number, fy: number, cx: number, cy: number, width_px: number, height_px: number, position: Point3D, right: Point3D, up: Point3D, forward: Point3D }} CameraModelJson
 * @typedef {{ group_name: string, poses: Pose3DJson[], camera_views?: CameraViewJson[], cameras?: CameraModelJson[], timestamp_ms: number }} PoseStreamMessage
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
    const camera_views = Array.isArray(data.camera_views)
      ? data.camera_views.map(normalizeCameraView)
      : [];
    const cameras = Array.isArray(data.cameras) ? data.cameras.map(normalizeCameraModel) : [];
    return {
      group_name: data.group_name,
      poses,
      camera_views,
      cameras,
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

function isValidPoint2D(k) {
  return (
    k != null &&
    typeof k === 'object' &&
    typeof k.x === 'number' &&
    typeof k.y === 'number' &&
    (typeof k.score === 'number' || k.score === undefined)
  );
}

function emptyPose() {
  return {
    keypoints: Array(17).fill(null),
    score: 0,
  };
}

function emptyPose2D() {
  return {
    keypoints: Array(17).fill(null),
    score: 0,
  };
}

/**
 * Normalize a 2D pose so keypoints is a 17-element array (null for missing).
 * @param {unknown} p
 * @returns {Pose2DJson}
 */
export function normalizePose2D(p) {
  if (!p || typeof p !== 'object') return emptyPose2D();
  const keypoints = Array.isArray(p.keypoints) ? p.keypoints : [];
  const arr = [];
  for (let i = 0; i < 17; i++) {
    const k = keypoints[i];
    arr.push(isValidPoint2D(k) ? { x: k.x, y: k.y, score: typeof k.score === 'number' ? k.score : 0 } : null);
  }
  return {
    keypoints: arr,
    score: typeof p.score === 'number' ? p.score : 0,
  };
}

/**
 * Normalize a camera view: camera_name and array of 2D poses.
 * @param {unknown} v
 * @returns {CameraViewJson}
 */
export function normalizeCameraView(v) {
  if (!v || typeof v !== 'object') return { camera_name: '', poses: [] };
  const poses = Array.isArray(v.poses) ? v.poses.map(normalizePose2D) : [];
  return {
    camera_name: typeof v.camera_name === 'string' ? v.camera_name : '',
    poses,
  };
}

/**
 * Normalize a camera model: camera_name + 3D vectors.
 * @param {unknown} c
 * @returns {CameraModelJson}
 */
export function normalizeCameraModel(c) {
  const empty = {
    camera_name: '',
    fx: 0,
    fy: 0,
    cx: 0,
    cy: 0,
    width_px: 0,
    height_px: 0,
    position: { x: 0, y: 0, z: 0, score: 1 },
    right: { x: 1, y: 0, z: 0, score: 1 },
    up: { x: 0, y: 1, z: 0, score: 1 },
    forward: { x: 0, y: 0, z: 1, score: 1 },
  };
  if (!c || typeof c !== 'object') return empty;
  const name = typeof c.camera_name === 'string' ? c.camera_name : '';
  return {
    camera_name: name,
    fx: typeof c.fx === 'number' ? c.fx : 0,
    fy: typeof c.fy === 'number' ? c.fy : 0,
    cx: typeof c.cx === 'number' ? c.cx : 0,
    cy: typeof c.cy === 'number' ? c.cy : 0,
    width_px: typeof c.width_px === 'number' ? c.width_px : 0,
    height_px: typeof c.height_px === 'number' ? c.height_px : 0,
    position: isValidPoint(c.position) ? c.position : empty.position,
    right: isValidPoint(c.right) ? c.right : empty.right,
    up: isValidPoint(c.up) ? c.up : empty.up,
    forward: isValidPoint(c.forward) ? c.forward : empty.forward,
  };
}

export { KEYPOINT_NAMES };
