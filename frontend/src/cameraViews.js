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
 * Build MJPEG stream URL for a camera. Returns empty string if base URL or group is missing.
 * @param {string} [videoBaseUrl]
 * @param {string} [groupName]
 * @param {string} cameraName
 * @returns {string}
 */
export function getCameraStreamUrl(videoBaseUrl, groupName, cameraName) {
  if (!videoBaseUrl || !groupName || !cameraName) return '';
  const base = videoBaseUrl.replace(/\/$/, '');
  return `${base}/api/camera/${encodeURIComponent(groupName)}/${encodeURIComponent(cameraName)}/stream`;
}

/**
 * @param {HTMLDivElement} container
 * @param {{ onViewClick?: (cameraName: string) => void, onKeypointClick?: (keypointIndex: number | null) => void, videoBaseUrl?: string | null, groupName?: string | null }} [options]
 * @returns {{ update: (camera_views: import('./poseMessage.js').CameraViewJson[]) => void, setHighlight: (cameraName: string | null) => void, setKeypointHighlight: (keypointIndex: number | null) => void, setVideoOptions: (videoBaseUrl: string | null, groupName: string | null) => void, setKnownCameras: (cameraNames: string[]) => void, dispose: () => void }}
 */
export function createCameraViews(container, options = {}) {
  const { onViewClick, onKeypointClick } = options;
  let videoBaseUrl = options.videoBaseUrl ?? null;
  let groupName = options.groupName ?? null;

  const grid = document.createElement('div');
  grid.className = 'camera-views-grid';
  container.appendChild(grid);

  /** @type {Map<string, { panel: HTMLDivElement, img: HTMLImageElement | null, canvas: HTMLCanvasElement, ctx: CanvasRenderingContext2D, lastKeypointPositions: { index: number, x: number, y: number }[] }>} */
  const panels = new Map();
  /** @type {import('./poseMessage.js').CameraViewJson[]} */
  let lastCameraViews = [];
  /** Camera names for the selected group (from ListCameras). Panels shown for these even when no pose data. */
  let knownCameraNames = [];
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
    const viewWrapper = document.createElement('div');
    viewWrapper.className = 'camera-view-wrapper';
    viewWrapper.style.position = 'relative';
    viewWrapper.style.width = `${CANVAS_WIDTH}px`;
    viewWrapper.style.height = `${CANVAS_HEIGHT}px`;
    const img = document.createElement('img');
    img.className = 'camera-view-mjpeg';
    img.alt = '';
    img.style.position = 'absolute';
    img.style.left = '0';
    img.style.top = '0';
    img.style.width = '100%';
    img.style.height = '100%';
    img.style.objectFit = 'contain';
    img.style.display = 'none';
    viewWrapper.appendChild(img);
    const canvas = document.createElement('canvas');
    canvas.className = 'camera-view-canvas';
    canvas.style.position = 'absolute';
    canvas.style.left = '0';
    canvas.style.top = '0';
    canvas.style.width = `${CANVAS_WIDTH}px`;
    canvas.style.height = `${CANVAS_HEIGHT}px`;
    canvas.style.pointerEvents = 'auto';
    viewWrapper.appendChild(canvas);
    panel.appendChild(viewWrapper);
    grid.appendChild(panel);
    const ctx = canvas.getContext('2d');
    if (!ctx) {
      panel.remove();
      return null;
    }
    const entry = { panel, img, canvas, ctx, lastKeypointPositions: [] };
    panels.set(cameraName, entry);
    updatePanelVideoSrc(entry, cameraName);
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

  const loggedUrls = new Set();
  function updatePanelVideoSrc(entry, cameraName) {
    const url = getCameraStreamUrl(videoBaseUrl, groupName, cameraName);
    if (url && entry.img) {
      entry.img.src = url;
      entry.img.style.display = '';
      if (!loggedUrls.has(url)) {
        loggedUrls.add(url);
        console.debug('[cameraViews] MJPEG stream URL:', url);
      }
    } else if (entry.img) {
      entry.img.src = '';
      entry.img.style.display = 'none';
    }
  }

  function setVideoOptions(baseUrl, group) {
    videoBaseUrl = baseUrl ?? null;
    groupName = group ?? null;
    panels.forEach((entry, cameraName) => {
      updatePanelVideoSrc(entry, cameraName);
    });
  }

  /** Set camera names for the selected group (from ListCameras). Panels are shown for these even when no pose data. */
  function setKnownCameras(cameraNames) {
    knownCameraNames = Array.isArray(cameraNames) ? cameraNames : [];
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
    const { canvas, ctx, img } = entry;
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
    const hasVideo = img && img.style.display !== 'none' && img.src;
    if (!hasVideo) {
      ctx.fillStyle = '#0d1117';
      ctx.fillRect(0, 0, w, h);
    }
    const poses = Array.isArray(view.poses) ? view.poses : [];
    const allKeypointPositions = [];
    for (const pose of poses) {
      if (pose && pose.keypoints && pose.keypoints.length) {
        for (let i = 0; i < pose.keypoints.length; i++) {
          const p = pose.keypoints[i];
          if (p) {
            allKeypointPositions.push({
              index: i,
              x: p.x * POSE_SCALE + POSE_OFFSET_X,
              y: p.y * POSE_SCALE + POSE_OFFSET_Y,
            });
          }
        }
        drawSkeleton(ctx, pose, POSE_SCALE, POSE_OFFSET_X, POSE_OFFSET_Y, highlightIndex);
      }
    }
    entry.lastKeypointPositions = allKeypointPositions;
    ctx.restore();
  }

  function update(camera_views) {
    // Show panels when either pose data (camera_views) or known cameras (from ListCameras) are present.
    const viewNames = new Set((camera_views || []).map((v) => v.camera_name));
    const allNames = new Set([...knownCameraNames, ...viewNames]);
    const viewsByCamera = new Map((camera_views || []).map((v) => [v.camera_name, v]));

    lastCameraViews = camera_views || [];

    for (const cameraName of allNames) {
      const entry = ensurePanel(cameraName);
      if (!entry) continue;
      const view = viewsByCamera.get(cameraName) || { camera_name: cameraName, poses: [] };
      drawView(entry, view, highlightedKeypointIndex);
    }

    // Remove panels for cameras no longer in known list or camera_views
    for (const [name, entry] of panels.entries()) {
      if (!allNames.has(name)) {
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

  /** On mobile, show only this camera's panel; pass null to show all. */
  function setMobileSingleCamera(cameraName) {
    panels.forEach((entry, name) => {
      entry.panel.classList.toggle('mobile-selected', name === cameraName);
    });
  }

  function dispose() {
    grid.remove();
    panels.clear();
  }

  return { update, setHighlight, setKeypointHighlight, setVideoOptions, setKnownCameras, setMobileSingleCamera, dispose };
}
