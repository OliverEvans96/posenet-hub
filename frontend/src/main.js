/**
 * Entry: connect to hub WebSocket, visualize poses, show info, camera views, group selector, admin control panel.
 */

import { createPoseStreamClient } from './websocketClient.js';
import { createAdminApi, AdminMethods } from './adminApi.js';
import { createPoseScene } from './scene.js';
import { createCameraViews } from './cameraViews.js';
import { KEYPOINT_NAMES } from './poseMessage.js';

const wsUrl = getWsUrl();
const connectionStatus = document.getElementById('connection-status');
const groupSelect = document.getElementById('group-select');
const cameraSelect = document.getElementById('control-camera-select');
const groupNameEl = document.getElementById('group-name');
const poseCountEl = document.getElementById('pose-count');
const timestampEl = document.getElementById('timestamp');
const messageRateEl = document.getElementById('message-rate');
const canvasContainer = document.getElementById('canvas-container');
const cameraViewsContainer = document.getElementById('camera-views-container');
const cameraStatusEl = document.getElementById('camera-status');

const OUT_CONTROL_ID = 'out-control';

const MOBILE_BREAKPOINT_PX = 768;

let selectedCameraName = null;
let selectedKeypointIndex = null;

/** Camera names for the selected group (used for mobile 2D menu). */
let knownCameraNames = [];

function updateStatusText() {
  if (!cameraStatusEl) return;
  if (selectedKeypointIndex != null && selectedKeypointIndex >= 0 && selectedKeypointIndex < KEYPOINT_NAMES.length) {
    cameraStatusEl.textContent = KEYPOINT_NAMES[selectedKeypointIndex];
  } else if (selectedCameraName) {
    cameraStatusEl.textContent = selectedCameraName;
  } else {
    cameraStatusEl.textContent = '—';
  }
}

let scene;
let cameraViews;

function selectCamera(cameraName) {
  selectedCameraName = cameraName ?? null;
  selectedKeypointIndex = null;
  updateStatusText();
  if (scene) {
    scene.setHighlight(selectedCameraName);
    scene.setKeypointHighlight(null);
  }
  if (cameraViews) {
    cameraViews.setHighlight(selectedCameraName);
    cameraViews.setKeypointHighlight(null);
  }
}

function selectKeypoint(keypointIndex) {
  selectedKeypointIndex = keypointIndex ?? null;
  selectedCameraName = null;
  updateStatusText();
  if (scene) {
    scene.setHighlight(null);
    scene.setKeypointHighlight(selectedKeypointIndex);
  }
  if (cameraViews) {
    cameraViews.setHighlight(null);
    cameraViews.setKeypointHighlight(selectedKeypointIndex);
  }
}

let lastMessageTime = 0;
const rateWindow = [];
const RATE_WINDOW_MS = 2000;

/** WebSocket URL from current page origin so mobile (and other devices) can connect when opening via host IP/hostname. */
function getWsUrl() {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.hostname || 'localhost';
  const wsPort = new URL(window.location.href).searchParams.get('ws') || '9001';
  return `${proto}//${host}:${wsPort}`;
}

/** Base URL for MJPEG camera streams from current page origin so mobile can access camera API on the same host. */
function getVideoBaseUrl() {
  const proto = window.location.protocol === 'https:' ? 'https:' : 'http:';
  const host = window.location.hostname || 'localhost';
  const videoPort = new URL(window.location.href).searchParams.get('video') || '9002';
  return `${proto}//${host}:${videoPort}`;
}

/** If set, only show pose stream for this group; empty = show all. */
let selectedGroupFilter = '';
/** Set once from first pose message when no group selected, so camera MJPEG URLs work. */
let hasSetVideoGroupFromPose = false;

let adminResponseHandler = null;

scene = createPoseScene(canvasContainer, { onCameraClick: selectCamera, onKeypointClick: selectKeypoint });
cameraViews = createCameraViews(cameraViewsContainer, {
  onViewClick: selectCamera,
  onKeypointClick: selectKeypoint,
  videoBaseUrl: getVideoBaseUrl(),
  groupName: selectedGroupFilter || null,
});

const client = createPoseStreamClient(wsUrl, {
  onPoseMessage(msg) {
    lastMessageTime = Date.now();
    rateWindow.push(lastMessageTime);
    while (rateWindow.length > 0 && lastMessageTime - rateWindow[0] > RATE_WINDOW_MS) rateWindow.shift();
    if (selectedGroupFilter !== '' && msg.group_name !== selectedGroupFilter) return;
    // When no group is selected, set video URLs from first pose message so camera feeds can load
    if (!selectedGroupFilter && msg.group_name && cameraViews?.setVideoOptions && !hasSetVideoGroupFromPose) {
      hasSetVideoGroupFromPose = true;
      cameraViews.setVideoOptions(getVideoBaseUrl(), msg.group_name);
    }
    scene.update(msg);
    scene.setKeypointHighlight(selectedKeypointIndex);
    scene.setHighlight(selectedCameraName);
    cameraViews.update(msg.camera_views ?? []);
    cameraViews.setKeypointHighlight(selectedKeypointIndex);
    cameraViews.setHighlight(selectedCameraName);
    updateInfo(msg);
  },
  onRawMessage(raw) {
    return adminResponseHandler ? adminResponseHandler(raw) : false;
  },
});

const adminApi = createAdminApi(
  (msg) => client.send(msg),
  (handler) => { adminResponseHandler = handler; }
);

function updateInfo(msg) {
  groupNameEl.textContent = msg.group_name || '—';
  poseCountEl.textContent = `${msg.poses?.length ?? 0} pose(s)`;
  timestampEl.textContent = msg.timestamp_ms
    ? new Date(msg.timestamp_ms).toISOString().slice(11, 23)
    : '—';
  const rate = rateWindow.length / (RATE_WINDOW_MS / 1000);
  messageRateEl.textContent = `${rate.toFixed(1)} msg/s`;
}

function updateConnectionStatus() {
  const state = client.getState();
  connectionStatus.textContent = state === 'open' ? 'Connected' : state === 'connecting' ? 'Connecting…' : 'Disconnected';
  connectionStatus.className = state === 'open' ? 'status connected' : 'status';
}

function getSelectedGroup() {
  return groupSelect ? groupSelect.value : '';
}

function getSelectedCamera() {
  return cameraSelect ? cameraSelect.value : '';
}

function refreshGroups() {
  adminApi.request(AdminMethods.ListGroups)
    .then((result) => {
      const names = result && result.group_names ? result.group_names : [];
      if (!groupSelect) return;
      const current = groupSelect.value;
      groupSelect.innerHTML = names.length === 0
        ? '<option value="">—</option>'
        : names.map((g) => `<option value="${escapeHtml(g)}"${g === current ? ' selected' : ''}>${escapeHtml(g)}</option>`).join('');
      if (names.length > 0 && !names.includes(current)) {
        groupSelect.value = names[0];
        selectedGroupFilter = names[0];
        if (cameraViews && cameraViews.setVideoOptions) {
          cameraViews.setVideoOptions(getVideoBaseUrl(), selectedGroupFilter);
        }
        refreshCameras();
      }
    })
    .catch((err) => {
      if (groupSelect) groupSelect.innerHTML = '<option value="">—</option>';
      console.error('ListGroups failed:', err);
    });
}

function refreshCameras() {
  const group = getSelectedGroup();
  if (!group) {
    if (cameraSelect) cameraSelect.innerHTML = '<option value="">(all)</option>';
    if (cameraViews && cameraViews.setKnownCameras) cameraViews.setKnownCameras([]);
    if (cameraViews) cameraViews.update([]);
    return;
  }
  adminApi.request(AdminMethods.ListCameras, { group_name: group })
    .then((result) => {
      const names = result && result.camera_names ? result.camera_names : [];
      knownCameraNames = names;
      updateMobileMenuCameras();
      if (cameraViews && cameraViews.setKnownCameras) {
        cameraViews.setKnownCameras(names);
        cameraViews.update([]);
      }
      if (!cameraSelect) return;
      const current = cameraSelect.value;
      cameraSelect.innerHTML = '<option value="">(all)</option>' +
        names.map((n) => `<option value="${escapeHtml(n)}"${n === current ? ' selected' : ''}>${escapeHtml(n)}</option>`).join('');
    })
    .catch(() => {
      knownCameraNames = [];
      updateMobileMenuCameras();
      if (cameraSelect) cameraSelect.innerHTML = '<option value="">(all)</option>';
      if (cameraViews && cameraViews.setKnownCameras) cameraViews.setKnownCameras([]);
      if (cameraViews) cameraViews.update([]);
    });
}

function updateMobileMenuCameras() {
  const container = document.getElementById('mobile-drawer-cameras');
  if (!container) return;
  container.innerHTML = '';
  for (const name of knownCameraNames) {
    const li = document.createElement('li');
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'mobile-drawer-item';
    btn.dataset.mobileView = '2d';
    btn.dataset.camera = name;
    btn.textContent = `2D: ${name}`;
    li.appendChild(btn);
    container.appendChild(li);
  }
}

function isMobileLayout() {
  return window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT_PX}px)`).matches;
}

/** Active mobile view: '3d' | '2d' (with mobileSelectedCamera set) | 'control' */
let mobileActiveView = '3d';
let mobileSelectedCamera = null;

function setMobileVisibleView(view, cameraName = null) {
  const canvasEl = document.getElementById('canvas-container');
  const controlEl = document.getElementById('control-pane');
  const cameraViewsEl = document.getElementById('camera-views-pane');
  if (!canvasEl || !controlEl || !cameraViewsEl) return;

  canvasEl.classList.remove('mobile-visible');
  controlEl.classList.remove('mobile-visible');
  cameraViewsEl.classList.remove('mobile-visible');

  if (view === '3d') {
    canvasEl.classList.add('mobile-visible');
  } else if (view === 'control') {
    controlEl.classList.add('mobile-visible');
  } else if (view === '2d' && cameraName) {
    cameraViewsEl.classList.add('mobile-visible');
    if (cameraViews && cameraViews.setMobileSingleCamera) {
      cameraViews.setMobileSingleCamera(cameraName);
    }
  }

  mobileActiveView = view;
  mobileSelectedCamera = cameraName || null;

  // Update aria-current on menu items
  document.querySelectorAll('.mobile-drawer-item').forEach((el) => {
    const viewAttr = el.dataset.mobileView;
    const camAttr = el.dataset.camera;
    const isActive = (viewAttr === '3d' && view === '3d') ||
      (viewAttr === 'control' && view === 'control') ||
      (viewAttr === '2d' && view === '2d' && camAttr === cameraName);
    el.setAttribute('aria-current', isActive ? 'true' : 'false');
  });
}

function openMobileDrawer() {
  const backdrop = document.getElementById('mobile-drawer-backdrop');
  const drawer = document.getElementById('mobile-drawer');
  const toggle = document.getElementById('mobile-menu-toggle');
  if (backdrop) backdrop.classList.add('is-open');
  if (drawer) {
    drawer.classList.add('is-open');
    drawer.setAttribute('aria-hidden', 'false');
  }
  if (toggle) {
    toggle.setAttribute('aria-expanded', 'true');
    toggle.setAttribute('aria-label', 'Close menu');
  }
}

function closeMobileDrawer() {
  const backdrop = document.getElementById('mobile-drawer-backdrop');
  const drawer = document.getElementById('mobile-drawer');
  const toggle = document.getElementById('mobile-menu-toggle');
  if (backdrop) backdrop.classList.remove('is-open');
  if (drawer) {
    drawer.classList.remove('is-open');
    drawer.setAttribute('aria-hidden', 'true');
  }
  if (toggle) {
    toggle.setAttribute('aria-expanded', 'false');
    toggle.setAttribute('aria-label', 'Open menu');
  }
}

function applyMobileLayout() {
  if (!isMobileLayout()) {
    document.getElementById('canvas-container')?.classList.remove('mobile-visible');
    document.getElementById('control-pane')?.classList.remove('mobile-visible');
    document.getElementById('camera-views-pane')?.classList.remove('mobile-visible');
    if (cameraViews && cameraViews.setMobileSingleCamera) cameraViews.setMobileSingleCamera(null);
    closeMobileDrawer();
    return;
  }
  setMobileVisibleView(mobileActiveView, mobileSelectedCamera);
}

function escapeHtml(s) {
  const div = document.createElement('div');
  div.textContent = s;
  return div.innerHTML;
}

function setOutput(id, text, isError = false) {
  const el = document.getElementById(id);
  if (el) {
    el.textContent = text;
    el.style.color = isError ? '#f85149' : '#8b949e';
  }
}

// Group selector: filter pose stream by selected group; refresh camera list and video stream URLs when group changes
if (groupSelect) {
  groupSelect.addEventListener('change', () => {
    selectedGroupFilter = groupSelect.value;
    if (!selectedGroupFilter) hasSetVideoGroupFromPose = false;
    if (cameraViews && cameraViews.setVideoOptions) {
      cameraViews.setVideoOptions(getVideoBaseUrl(), selectedGroupFilter || null);
    }
    refreshCameras();
  });
}

// Control pane toggle
const controlPane = document.getElementById('control-pane');
const controlPaneToggle = document.getElementById('control-pane-toggle');
if (controlPane && controlPaneToggle) {
  controlPaneToggle.addEventListener('click', () => {
    const collapsed = controlPane.classList.toggle('collapsed');
    controlPaneToggle.setAttribute('aria-expanded', String(!collapsed));
    controlPaneToggle.textContent = collapsed ? '+' : '−';
    controlPaneToggle.title = collapsed ? 'Show control panel' : 'Hide control panel';
  });
}

// Camera views pane toggle
const cameraViewsPane = document.getElementById('camera-views-pane');
const cameraViewsToggle = document.getElementById('camera-views-toggle');
if (cameraViewsPane && cameraViewsToggle) {
  cameraViewsToggle.addEventListener('click', () => {
    const collapsed = cameraViewsPane.classList.toggle('collapsed');
    cameraViewsToggle.setAttribute('aria-expanded', String(!collapsed));
    cameraViewsToggle.textContent = collapsed ? '+' : '−';
    cameraViewsToggle.title = collapsed ? 'Show camera views' : 'Hide camera views';
  });
}

// Mobile: hamburger, drawer, view switching
const mobileMenuToggle = document.getElementById('mobile-menu-toggle');
const mobileDrawerBackdrop = document.getElementById('mobile-drawer-backdrop');
const mobileDrawer = document.getElementById('mobile-drawer');
const mobileDrawerClose = document.getElementById('mobile-drawer-close');

if (mobileMenuToggle) {
  mobileMenuToggle.addEventListener('click', () => {
    if (!isMobileLayout()) return;
    openMobileDrawer();
  });
}
if (mobileDrawerBackdrop) {
  mobileDrawerBackdrop.addEventListener('click', closeMobileDrawer);
}
if (mobileDrawerClose) {
  mobileDrawerClose.addEventListener('click', closeMobileDrawer);
}
if (mobileDrawer) {
  mobileDrawer.addEventListener('click', (e) => {
    const item = e.target.closest('.mobile-drawer-item');
    if (!item || !isMobileLayout()) return;
    const view = item.dataset.mobileView;
    const camera = item.dataset.camera || null;
    if (view === '3d') {
      setMobileVisibleView('3d');
    } else if (view === 'control') {
      setMobileVisibleView('control');
    } else if (view === '2d' && camera) {
      setMobileVisibleView('2d', camera);
    }
    closeMobileDrawer();
  });
}

window.addEventListener('resize', applyMobileLayout);
applyMobileLayout();

// Admin buttons
function bindAdmin(id, method, paramsFn, outputId) {
  const btn = document.getElementById(id);
  if (!btn) return;
  btn.addEventListener('click', () => {
    btn.disabled = true;
    const params = typeof paramsFn === 'function' ? paramsFn() : paramsFn;
    adminApi.request(method, params)
      .then((result) => {
        setOutput(outputId, JSON.stringify(result, null, 2));
      })
      .catch((err) => {
        setOutput(outputId, err.message || String(err), true);
      })
      .finally(() => { btn.disabled = false; });
  });
}

// List groups: also refresh group selector dropdown (no "All", default to first)
const btnListGroups = document.getElementById('btn-list-groups');
if (btnListGroups) {
  btnListGroups.addEventListener('click', () => {
    btnListGroups.disabled = true;
    adminApi.request(AdminMethods.ListGroups, {})
      .then((result) => {
        const names = result && result.group_names ? result.group_names : [];
        setOutput(OUT_CONTROL_ID, JSON.stringify(result, null, 2));
      if (groupSelect) {
        const current = groupSelect.value;
        groupSelect.innerHTML = names.length === 0
          ? '<option value="">—</option>'
          : names.map((g) => `<option value="${escapeHtml(g)}"${g === current ? ' selected' : ''}>${escapeHtml(g)}</option>`).join('');
        if (names.length > 0 && !names.includes(current)) {
          groupSelect.value = names[0];
          selectedGroupFilter = names[0];
          if (cameraViews && cameraViews.setVideoOptions) {
            cameraViews.setVideoOptions(getVideoBaseUrl(), names[0]);
          }
          refreshCameras();
        }
      }
      })
      .catch((err) => {
        setOutput(OUT_CONTROL_ID, err.message || String(err), true);
      })
      .finally(() => { btnListGroups.disabled = false; });
  });
}
// List cameras: show result and refresh camera dropdown from result
const btnListCameras = document.getElementById('btn-list-cameras');
if (btnListCameras) {
  btnListCameras.addEventListener('click', () => {
    btnListCameras.disabled = true;
    adminApi.request(AdminMethods.ListCameras, { group_name: getSelectedGroup() })
      .then((result) => {
        setOutput(OUT_CONTROL_ID, JSON.stringify(result, null, 2));
        const names = result && result.camera_names ? result.camera_names : [];
        knownCameraNames = names;
        updateMobileMenuCameras();
        if (cameraSelect) {
          const current = cameraSelect.value;
          cameraSelect.innerHTML = '<option value="">(all)</option>' +
            names.map((n) => `<option value="${escapeHtml(n)}"${n === current ? ' selected' : ''}>${escapeHtml(n)}</option>`).join('');
        }
      })
      .catch((err) => {
        setOutput(OUT_CONTROL_ID, err.message || String(err), true);
      })
      .finally(() => { btnListCameras.disabled = false; });
  });
}
bindAdmin('btn-get-camera-info', AdminMethods.GetCameraInfo, () => ({ group_name: getSelectedGroup(), camera_name: getSelectedCamera() }), OUT_CONTROL_ID);

bindAdmin('btn-stream-start', AdminMethods.StreamControl, () => ({
  group_name: getSelectedGroup(),
  command: { start: { with_pose: true, with_image: true, fps: 10 } },
}), OUT_CONTROL_ID);
bindAdmin('btn-stream-stop', AdminMethods.StreamControl, () => ({
  group_name: getSelectedGroup(),
  command: { stop: {} },
}), OUT_CONTROL_ID);

bindAdmin('btn-take-snapshots', AdminMethods.TakeSnapshots, () => ({
  group_name: getSelectedGroup(),
  camera_name: getSelectedCamera(),
  with_pose: true,
  with_image: false,
  want_pose3d: false,
}), OUT_CONTROL_ID);
bindAdmin('btn-get-current', AdminMethods.GetCurrent, () => ({
  group_name: getSelectedGroup(),
  camera_name: getSelectedCamera(),
}), OUT_CONTROL_ID);

bindAdmin('btn-calibrate', AdminMethods.Calibrate, () => ({
  group_name: getSelectedGroup(),
  camera_name: getSelectedCamera(),
  do_extrinsic: true,
  do_intrinsic: false,
}), OUT_CONTROL_ID);
bindAdmin('btn-ping', AdminMethods.Ping, () => ({
  group_name: getSelectedGroup(),
  camera_name: getSelectedCamera(),
  timeout_secs: 5,
}), OUT_CONTROL_ID);

bindAdmin('btn-update-cameras', AdminMethods.UpdateCameras, () => ({
  group_name: getSelectedGroup(),
  camera_name: getSelectedCamera(),
}), OUT_CONTROL_ID);

setInterval(() => {
  updateConnectionStatus();
  while (rateWindow.length > 0 && Date.now() - rateWindow[0] > RATE_WINDOW_MS) rateWindow.shift();
  const rate = rateWindow.length / (RATE_WINDOW_MS / 1000);
  messageRateEl.textContent = `${rate.toFixed(1)} msg/s`;
}, 300);

client.connect();
updateConnectionStatus();

// Refresh groups when connected
const checkConnected = setInterval(() => {
  if (client.getState() === 'open') {
    refreshGroups();
    clearInterval(checkConnected);
  }
}, 500);
setTimeout(() => clearInterval(checkConnected), 30000);

function animate() {
  requestAnimationFrame(animate);
  scene.render();
}
animate();
