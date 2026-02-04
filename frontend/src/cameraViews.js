/**
 * Renders per-camera 2D pose views (skeleton overlays) in a grid of canvases.
 */

import { getVisibleEdges } from './poseSkeleton.js';

const BONE_COLOR = '#00ccff';
const JOINT_COLOR = '#ff6600';
const JOINT_RADIUS = 4;

/** Fixed size of each camera view canvas (logical pixels). */
const CANVAS_WIDTH = 320;
const CANVAS_HEIGHT = 240;

/** Fixed scale: smaller value = larger field of view (more of the scene visible). */
const POSE_SCALE = 0.25;

/**
 * @param {HTMLDivElement} container
 * @returns {{ update: (camera_views: import('./poseMessage.js').CameraViewJson[]) => void, dispose: () => void }}
 */
export function createCameraViews(container) {
  const grid = document.createElement('div');
  grid.className = 'camera-views-grid';
  container.appendChild(grid);

  /** @type {Map<string, { panel: HTMLDivElement, canvas: HTMLCanvasElement, ctx: CanvasRenderingContext2D }>} */
  const panels = new Map();

  function ensurePanel(cameraName) {
    if (panels.has(cameraName)) return panels.get(cameraName);
    const panel = document.createElement('div');
    panel.className = 'camera-view-panel';
    const title = document.createElement('div');
    title.className = 'camera-view-title';
    title.textContent = cameraName;
    panel.appendChild(title);
    const canvas = document.createElement('canvas');
    canvas.className = 'camera-view-canvas';
    canvas.style.width = `${CANVAS_WIDTH}px`;
    canvas.style.height = `${CANVAS_HEIGHT}px`;
    panel.appendChild(canvas);
    grid.appendChild(panel);
    const ctx = canvas.getContext('2d');
    if (!ctx) {
      panel.remove();
      return null;
    }
    const entry = { panel, canvas, ctx };
    panels.set(cameraName, entry);
    return entry;
  }

  function clearPanel(entry) {
    const { canvas, ctx } = entry;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
  }

  function drawSkeleton(ctx, pose, scale, offsetX, offsetY) {
    const keypoints = pose.keypoints;
    const edges = getVisibleEdges(keypoints);

    ctx.strokeStyle = BONE_COLOR;
    ctx.lineWidth = 2;
    for (const [a, b] of edges) {
      const pa = keypoints[a];
      const pb = keypoints[b];
      if (!pa || !pb) continue;
      ctx.beginPath();
      ctx.moveTo(pa.x * scale + offsetX, pa.y * scale + offsetY);
      ctx.lineTo(pb.x * scale + offsetX, pb.y * scale + offsetY);
      ctx.stroke();
    }

    ctx.fillStyle = JOINT_COLOR;
    for (const p of keypoints) {
      if (!p) continue;
      ctx.beginPath();
      ctx.arc(p.x * scale + offsetX, p.y * scale + offsetY, JOINT_RADIUS, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  function update(camera_views) {
    if (!camera_views || camera_views.length === 0) {
      panels.forEach((entry) => clearPanel(entry));
      return;
    }

    const w = CANVAS_WIDTH;
    const h = CANVAS_HEIGHT;
    const offsetX = w / 2;
    const offsetY = h / 2;

    for (const view of camera_views) {
      const entry = ensurePanel(view.camera_name);
      if (!entry) continue;

      const { canvas, ctx } = entry;
      const dpr = window.devicePixelRatio || 1;
      if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
        canvas.width = w * dpr;
        canvas.height = h * dpr;
        canvas.style.width = `${w}px`;
        canvas.style.height = `${h}px`;
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      }

      ctx.save();
      ctx.beginPath();
      ctx.rect(0, 0, w, h);
      ctx.clip();
      ctx.clearRect(0, 0, w, h);
      ctx.fillStyle = '#0d1117';
      ctx.fillRect(0, 0, w, h);

      const pose = view.poses[0];
      if (!pose || !pose.keypoints.length) {
        ctx.restore();
        continue;
      }

      const valid = pose.keypoints.filter((p) => p != null);
      if (valid.length === 0) {
        ctx.restore();
        continue;
      }

      drawSkeleton(ctx, pose, POSE_SCALE, offsetX, offsetY);
      ctx.restore();
    }

    // Remove panels for cameras no longer present
    const currentNames = new Set(camera_views.map((v) => v.camera_name));
    for (const [name, entry] of panels.entries()) {
      if (!currentNames.has(name)) {
        entry.panel.remove();
        panels.delete(name);
      }
    }
  }

  function dispose() {
    grid.remove();
    panels.clear();
  }

  return { update, dispose };
}
