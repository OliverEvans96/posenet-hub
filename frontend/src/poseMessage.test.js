import { describe, it, expect } from 'vitest';
import {
  parsePoseStreamMessage,
  normalizePose,
  normalizePose2D,
  normalizeCameraView,
  normalizeCameraModel,
  KEYPOINT_NAMES,
} from './poseMessage.js';

describe('parsePoseStreamMessage', () => {
  it('returns null for empty or non-string input', () => {
    expect(parsePoseStreamMessage('')).toBeNull();
    expect(parsePoseStreamMessage('   ')).toBeNull();
    expect(parsePoseStreamMessage(null)).toBeNull();
    expect(parsePoseStreamMessage(123)).toBeNull();
  });

  it('returns null for invalid JSON', () => {
    expect(parsePoseStreamMessage('not json')).toBeNull();
    expect(parsePoseStreamMessage('{')).toBeNull();
  });

  it('returns null when group_name is missing or not a string', () => {
    expect(parsePoseStreamMessage('{}')).toBeNull();
    expect(parsePoseStreamMessage('{"poses":[]}')).toBeNull();
    expect(parsePoseStreamMessage('{"group_name":123,"poses":[]}')).toBeNull();
  });

  it('returns null when poses is not an array', () => {
    expect(parsePoseStreamMessage('{"group_name":"g","poses":null}')).toBeNull();
    expect(parsePoseStreamMessage('{"group_name":"g","poses":{}}')).toBeNull();
  });

  it('parses a minimal valid message', () => {
    const raw = '{"group_name":"test_group","poses":[],"timestamp_ms":1000}';
    const msg = parsePoseStreamMessage(raw);
    expect(msg).not.toBeNull();
    expect(msg.group_name).toBe('test_group');
    expect(msg.poses).toEqual([]);
    expect(msg.timestamp_ms).toBe(1000);
  });

  it('parses cameras with intrinsics', () => {
    const raw = JSON.stringify({
      group_name: 'g',
      poses: [],
      cameras: [
        {
          camera_name: 'cam0',
          fx: 800,
          fy: 810,
          cx: 320,
          cy: 240,
          width_px: 640,
          height_px: 480,
          position: { x: 1, y: 2, z: 3, score: 1 },
          right: { x: 1, y: 0, z: 0, score: 1 },
          up: { x: 0, y: 1, z: 0, score: 1 },
          forward: { x: 0, y: 0, z: 1, score: 1 },
        },
      ],
    });
    const msg = parsePoseStreamMessage(raw);
    expect(msg).not.toBeNull();
    expect(msg.cameras).toHaveLength(1);
    expect(msg.cameras[0].camera_name).toBe('cam0');
    expect(msg.cameras[0].fx).toBe(800);
    expect(msg.cameras[0].width_px).toBe(640);
  });

  it('parses a message with one pose and keypoints', () => {
    const raw = JSON.stringify({
      group_name: 'cam1',
      poses: [
        {
          keypoints: [
            { x: 0, y: 0, z: 0, score: 1 },
            { x: 0.1, y: 0, z: 0, score: 0.9 },
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
          ],
          score: 0.95,
        },
      ],
      timestamp_ms: 1234567890,
    });
    const msg = parsePoseStreamMessage(raw);
    expect(msg).not.toBeNull();
    expect(msg.group_name).toBe('cam1');
    expect(msg.poses).toHaveLength(1);
    expect(msg.poses[0].keypoints[0]).toEqual({ x: 0, y: 0, z: 0, score: 1 });
    expect(msg.poses[0].keypoints[1]).toEqual({ x: 0.1, y: 0, z: 0, score: 0.9 });
    expect(msg.poses[0].score).toBe(0.95);
    expect(msg.timestamp_ms).toBe(1234567890);
  });
});

describe('normalizePose', () => {
  it('returns empty pose for null or non-object', () => {
    const empty = { keypoints: Array(17).fill(null), score: 0 };
    expect(normalizePose(null)).toEqual(empty);
    expect(normalizePose(undefined)).toEqual(empty);
    expect(normalizePose('')).toEqual(empty);
  });

  it('pads keypoints to 17 elements', () => {
    const p = normalizePose({ keypoints: [{ x: 1, y: 2, z: 3, score: 1 }], score: 0.5 });
    expect(p.keypoints).toHaveLength(17);
    expect(p.keypoints[0]).toEqual({ x: 1, y: 2, z: 3, score: 1 });
    expect(p.keypoints[1]).toBeNull();
    expect(p.score).toBe(0.5);
  });

  it('filters invalid keypoints to null', () => {
    const p = normalizePose({
      keypoints: [
        { x: 1, y: 2, z: 3, score: 1 },
        { x: 0 }, // missing y,z,score
        null,
      ],
      score: 0,
    });
    expect(p.keypoints[0]).toEqual({ x: 1, y: 2, z: 3, score: 1 });
    expect(p.keypoints[1]).toBeNull();
  });
});

describe('normalizePose2D', () => {
  it('returns empty 2D pose for null or non-object', () => {
    const empty = { keypoints: Array(17).fill(null), score: 0 };
    expect(normalizePose2D(null)).toEqual(empty);
    expect(normalizePose2D(undefined)).toEqual(empty);
  });

  it('normalizes 2D keypoints (x, y, score)', () => {
    const p = normalizePose2D({
      keypoints: [{ x: 100, y: 200, score: 0.8 }],
      score: 0.8,
    });
    expect(p.keypoints).toHaveLength(17);
    expect(p.keypoints[0]).toEqual({ x: 100, y: 200, score: 0.8 });
    expect(p.keypoints[1]).toBeNull();
    expect(p.score).toBe(0.8);
  });

  it('filters invalid 2D keypoints to null', () => {
    const p = normalizePose2D({
      keypoints: [{ x: 1, y: 2 }, { x: 0 }],
      score: 0,
    });
    expect(p.keypoints[0]).toEqual({ x: 1, y: 2, score: 0 });
    expect(p.keypoints[1]).toBeNull();
  });
});

describe('normalizeCameraView', () => {
  it('returns empty view for null or non-object', () => {
    expect(normalizeCameraView(null)).toEqual({ camera_name: '', poses: [] });
    expect(normalizeCameraView(undefined)).toEqual({ camera_name: '', poses: [] });
  });

  it('normalizes camera_name and poses', () => {
    const v = normalizeCameraView({
      camera_name: 'front_cam',
      poses: [{ keypoints: [{ x: 0, y: 0, score: 1 }], score: 1 }],
    });
    expect(v.camera_name).toBe('front_cam');
    expect(v.poses).toHaveLength(1);
    expect(v.poses[0].keypoints[0]).toEqual({ x: 0, y: 0, score: 1 });
  });
});

describe('normalizeCameraModel', () => {
  it('normalizes intrinsics fields', () => {
    const c = normalizeCameraModel({
      camera_name: 'c',
      fx: 500,
      fy: 600,
      cx: 100,
      cy: 200,
      width_px: 200,
      height_px: 400,
      position: { x: 0, y: 0, z: 0, score: 1 },
      right: { x: 1, y: 0, z: 0, score: 1 },
      up: { x: 0, y: 1, z: 0, score: 1 },
      forward: { x: 0, y: 0, z: 1, score: 1 },
    });
    expect(c.fx).toBe(500);
    expect(c.height_px).toBe(400);
  });
});

describe('KEYPOINT_NAMES', () => {
  it('has 17 keypoint names', () => {
    expect(KEYPOINT_NAMES).toHaveLength(17);
    expect(KEYPOINT_NAMES[0]).toBe('nose');
    expect(KEYPOINT_NAMES[16]).toBe('right_ankle');
  });
});
