/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createCameraViews, getCameraStreamUrl } from './cameraViews.js';

describe('getCameraStreamUrl', () => {
  it('returns empty string when videoBaseUrl is missing', () => {
    expect(getCameraStreamUrl('', 'default', 'cam1')).toBe('');
    expect(getCameraStreamUrl(null, 'default', 'cam1')).toBe('');
    expect(getCameraStreamUrl(undefined, 'default', 'cam1')).toBe('');
  });

  it('returns empty string when groupName is missing', () => {
    expect(getCameraStreamUrl('http://localhost:9002', '', 'cam1')).toBe('');
  });

  it('returns empty string when cameraName is missing', () => {
    expect(getCameraStreamUrl('http://localhost:9002', 'default', '')).toBe('');
  });

  it('builds URL with encoded path segments', () => {
    expect(getCameraStreamUrl('http://localhost:9002', 'default', 'cam1')).toBe(
      'http://localhost:9002/api/camera/default/cam1/stream'
    );
  });

  it('strips trailing slash from videoBaseUrl', () => {
    expect(getCameraStreamUrl('http://localhost:9002/', 'grp', 'my-cam')).toBe(
      'http://localhost:9002/api/camera/grp/my-cam/stream'
    );
  });

  it('encodes special characters in group and camera name', () => {
    const url = getCameraStreamUrl('http://host', 'my group', 'cam/1');
    expect(url).toContain('api/camera/');
    expect(url).toContain('/stream');
    expect(url).toContain(encodeURIComponent('my group'));
    expect(url).toContain(encodeURIComponent('cam/1'));
  });
});

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

  it('returns update, setVideoOptions, setKnownCameras, and dispose', () => {
    const views = createCameraViews(container);
    expect(views).toHaveProperty('update');
    expect(views).toHaveProperty('setVideoOptions');
    expect(views).toHaveProperty('setKnownCameras');
    expect(views).toHaveProperty('dispose');
    expect(typeof views.update).toBe('function');
    expect(typeof views.setVideoOptions).toBe('function');
    expect(typeof views.setKnownCameras).toBe('function');
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

  it('setKnownCameras then update([]) shows panels for known cameras (no pose data)', () => {
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
      views.setKnownCameras(['cam1', 'cam2']);
      views.update([]);
      const panels = container.querySelectorAll('.camera-view-panel');
      expect(panels.length).toBe(2);
      const titles = [...panels].map((p) => p.querySelector('.camera-view-title')?.textContent);
      expect(titles).toContain('cam1');
      expect(titles).toContain('cam2');
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
