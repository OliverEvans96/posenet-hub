import { describe, it, expect } from 'vitest';
import { SKELETON_EDGES, getVisibleEdges } from './poseSkeleton.js';

describe('SKELETON_EDGES', () => {
  it('contains pairs of keypoint indices', () => {
    expect(SKELETON_EDGES.length).toBeGreaterThan(0);
    for (const [a, b] of SKELETON_EDGES) {
      expect(typeof a).toBe('number');
      expect(typeof b).toBe('number');
      expect(a).toBeGreaterThanOrEqual(0);
      expect(a).toBeLessThan(17);
      expect(b).toBeGreaterThanOrEqual(0);
      expect(b).toBeLessThan(17);
    }
  });
});

describe('getVisibleEdges', () => {
  it('returns no edges when all keypoints are null', () => {
    const keypoints = Array(17).fill(null);
    expect(getVisibleEdges(keypoints)).toEqual([]);
  });

  it('returns edges only for pairs where both keypoints are present', () => {
    const keypoints = Array(17).fill(null);
    keypoints[0] = { x: 0, y: 0, z: 0 };
    keypoints[1] = { x: 0.1, y: 0, z: 0 };
    keypoints[2] = { x: -0.1, y: 0, z: 0 };
    const edges = getVisibleEdges(keypoints);
    expect(edges).toContainEqual([0, 1]);
    expect(edges).toContainEqual([0, 2]);
    expect(edges.some(([a, b]) => a === 3 || b === 3)).toBe(false);
  });

  it('returns subset of SKELETON_EDGES when some keypoints missing', () => {
    const keypoints = Array(17).fill(null);
    keypoints[5] = { x: 0, y: 0.5, z: 0 };
    keypoints[6] = { x: 0.2, y: 0.5, z: 0 };
    const edges = getVisibleEdges(keypoints);
    expect(edges).toContainEqual([5, 6]);
  });
});
