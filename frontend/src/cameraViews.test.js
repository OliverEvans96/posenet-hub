/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createCameraViews } from './cameraViews.js';

describe('createCameraViews', () => {
  /** @type {HTMLDivElement} */
  let container;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    if (container.parentNode) container.remove();
  });

  it('returns update and dispose', () => {
    const views = createCameraViews(container);
    expect(views).toHaveProperty('update');
    expect(views).toHaveProperty('dispose');
    expect(typeof views.update).toBe('function');
    expect(typeof views.dispose).toBe('function');
    views.dispose();
  });

  it('update with empty array does not throw', () => {
    const views = createCameraViews(container);
    expect(() => views.update([])).not.toThrow();
    views.dispose();
  });

  it('update with one camera view adds one panel', () => {
    const origGetContext = HTMLCanvasElement.prototype.getContext;
    const mock2D = {
      clearRect: () => {},
      fillRect: () => {},
      fillStyle: '',
      strokeStyle: '',
      lineWidth: 0,
      beginPath: () => {},
      moveTo: () => {},
      lineTo: () => {},
      stroke: () => {},
      arc: () => {},
      fill: () => {},
      setTransform: () => {},
      save: () => {},
      restore: () => {},
      rect: () => {},
      clip: () => {},
    };
    HTMLCanvasElement.prototype.getContext = function (type) {
      if (type === '2d') return mock2D;
      return origGetContext?.call(this, type) ?? null;
    };
    try {
      const views = createCameraViews(container);
      views.update([
        {
          camera_name: 'test_cam',
          poses: [
            {
              keypoints: [
                { x: 100, y: 100, score: 1 },
                ...Array(16).fill(null),
              ],
              score: 1,
            },
          ],
        },
      ]);
      const grid = container.querySelector('.camera-views-grid');
      expect(grid).not.toBeNull();
      const panels = container.querySelectorAll('.camera-view-panel');
      expect(panels.length).toBe(1);
      expect(panels[0].querySelector('.camera-view-title')?.textContent).toBe('test_cam');
      views.dispose();
    } finally {
      HTMLCanvasElement.prototype.getContext = origGetContext;
    }
  });

  it('dispose removes grid from container', () => {
    const views = createCameraViews(container);
    views.update([]);
    expect(container.querySelector('.camera-views-grid')).not.toBeNull();
    views.dispose();
    expect(container.querySelector('.camera-views-grid')).toBeNull();
  });
});
