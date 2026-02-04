/**
 * Renders per-camera 2D pose views (skeleton overlays) in a grid of canvases.
 */

import { getVisibleEdges } from './poseSkeleton.js';

const BONE_COLOR = '#00ccff';
const JOINT_COLOR = '#ff6600';
const JOINT_HIGHLIGHT_COLOR = '#58a6ff';
const JOINT_RADIUS = 4;
const KEYPOINT_HIT_RADIUS = 15;

/** Fixed size of each camera view canvas (logical pixels). */
const CANVAS_WIDTH = 320;
const CANVAS_HEIGHT = 240;

/**
 * Source image size used by synthetic cameras (and assumed for 2D pose coords).
 * Backend projects into [0, SOURCE_IMAGE_WIDTH] x [0, SOURCE_IMAGE_HEIGHT].
 * We map that extent to the canvas so (0,0) is top-left and (SOURCE_*, SOURCE_*) is bottom-right.
 */
const SOURCE_IMAGE_WIDTH = 640;
const SOURCE_IMAGE_HEIGHT = 480;

/**
 * Scale from source image pixels to canvas pixels (full image [0,640]x[0,480] fills canvas).
 * Same factor for x and y so aspect ratio is preserved.
 */
const POSE_SCALE = CANVAS_WIDTH / SOURCE_IMAGE_WIDTH;
const POSE_OFFSET_X = 0;
const POSE_OFFSET_Y = 0;

const HIGHLIGHT_CLASS = 'camera-view-panel-highlight';

/**
 * @param {HTMLDivElement} container
 * @param {{ onViewClick?: (cameraName: string) => void, onKeypointClick?: (keypointIndex: number | null) => void }} [options]
 * @returns {{ update: (camera_views: import('./poseMessage.js').CameraViewJson[]) => void, setHighlight: (cameraName: string | null) => void, setKeypointHighlight: (keypointIndex: number | null) => void, dispose: () => void }}
 */
export function createCameraViews(container, options = {}) {
  const { onViewClick, onKeypointClick } = options;
  const grid = document.createElement('div');
  grid.className = 'camera-views-grid';
  container.appendChild(grid);

  /** @type {Map<string, { panel: HTMLDivElement, canvas: HTMLCanvasElement, ctx: CanvasRenderingContext2D, lastKeypointPositions: { index: number, x: number, y: number }[] }>} */
  const panels = new Map();
  /** @type {import('./poseMessage.js').CameraViewJson[]} */
  let lastCameraViews = [];
  let highlightedKeypointIndex = null;

  function ensurePanel(cameraName) {
    if (panels.has(cameraName)) return panels.get(cameraName);
    const panel = document.createElement('div');
    panel.className = 'camera-view-panel';
    panel.dataset.cameraName = cameraName;
    panel.setAttribute('role', 'button');
    panel.setAttribute('tabindex', '0');
    panel.setAttribute('aria-label', `Camera ${cameraName}`);
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
    const entry = { panel, canvas, ctx, lastKeypointPositions: [] };
    panels.set(cameraName, entry);
    if (onViewClick) {
      const handlePanelClick = () => onViewClick(cameraName);
      panel.addEventListener('click', handlePanelClick);
      panel.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handlePanelClick(); } });
    }
    if (onKeypointClick) {
      canvas.addEventListener('click', (e) => {
        e.stopPropagation();
        const rect = canvas.getBoundingClientRect();
        const cx = ((e.clientX - rect.left) / rect.width) * CANVAS_WIDTH;
        const cy = ((e.clientY - rect.top) / rect.height) * CANVAS_HEIGHT;
        const positions = entry.lastKeypointPositions;
        let hit = null;
        for (const pos of positions) {
          const d = Math.hypot(cx - pos.x, cy - pos.y);
          if (d <= KEYPOINT_HIT_RADIUS) { hit = pos.index; break; }
        }
        onKeypointClick(hit != null ? hit : null);
      });
    }
    return entry;
  }

  function setHighlight(cameraName) {
    panels.forEach((entry, name) => {
      entry.panel.classList.toggle(HIGHLIGHT_CLASS, name === cameraName);
    });
  }

  function clearPanel(entry) {
    const { canvas, ctx } = entry;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
  }

  /**
   * @param {CanvasRenderingContext2D} ctx
   * @param {import('./poseMessage.js').Pose2DJson} pose
   * @param {number} scale
   * @param {number} offsetX
   * @param {number} offsetY
   * @param {number | null} [highlightedKeypointIndex]
   */
  function drawSkeleton(ctx, pose, scale, offsetX, offsetY, highlightedKeypointIndex = null) {
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

    for (let i = 0; i < keypoints.length; i++) {
      const p = keypoints[i];
      if (!p) continue;
      const x = p.x * scale + offsetX;
      const y = p.y * scale + offsetY;
      ctx.fillStyle = i === highlightedKeypointIndex ? JOINT_HIGHLIGHT_COLOR : JOINT_COLOR;
      ctx.beginPath();
      ctx.arc(x, y, JOINT_RADIUS, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  function drawView(entry, view, highlightIndex) {
    const { canvas, ctx } = entry;
    const w = CANVAS_WIDTH;
    const h = CANVAS_HEIGHT;
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
    if (pose && pose.keypoints.length) {
      const keypoints = pose.keypoints;
      entry.lastKeypointPositions = keypoints
        .map((p, i) => (p ? { index: i, x: p.x * POSE_SCALE + POSE_OFFSET_X, y: p.y * POSE_SCALE + POSE_OFFSET_Y } : null))
        .filter((p) => p != null);
      drawSkeleton(ctx, pose, POSE_SCALE, POSE_OFFSET_X, POSE_OFFSET_Y, highlightIndex);
    } else {
      entry.lastKeypointPositions = [];
    }
    ctx.restore();
  }

  function update(camera_views) {
    if (!camera_views || camera_views.length === 0) {
      lastCameraViews = [];
      panels.forEach((entry) => clearPanel(entry));
      return;
    }
    lastCameraViews = camera_views;
    const w = CANVAS_WIDTH;
    const h = CANVAS_HEIGHT;

    for (const view of camera_views) {
      const entry = ensurePanel(view.camera_name);
      if (!entry) continue;
      drawView(entry, view, highlightedKeypointIndex);
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

  function setKeypointHighlight(keypointIndex) {
    highlightedKeypointIndex = keypointIndex;
    if (lastCameraViews.length === 0) return;
    for (const view of lastCameraViews) {
      const entry = panels.get(view.camera_name);
      if (entry) drawView(entry, view, highlightedKeypointIndex);
    }
  }

  function dispose() {
    grid.remove();
    panels.clear();
  }

  return { update, setHighlight, setKeypointHighlight, dispose };
}
