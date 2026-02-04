/**
 * Entry: connect to hub WebSocket, visualize poses, show info, camera views, group selector, admin control panel.
 */

import { createPoseStreamClient } from './websocketClient.js';
import { createAdminApi, AdminMethods } from './adminApi.js';
import { createPoseScene } from './scene.js';
import { createCameraViews } from './cameraViews.js';

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

const OUT_CONTROL_ID = 'out-control';

let lastMessageTime = 0;
const rateWindow = [];
const RATE_WINDOW_MS = 2000;

function getWsUrl() {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.hostname || 'localhost';
  const wsPort = new URL(window.location.href).searchParams.get('ws') || '9001';
  return `${proto}//${host}:${wsPort}`;
}

/** If set, only show pose stream for this group; empty = show all. */
let selectedGroupFilter = '';

let adminResponseHandler = null;
const client = createPoseStreamClient(wsUrl, {
  onPoseMessage(msg) {
    lastMessageTime = Date.now();
    rateWindow.push(lastMessageTime);
    while (rateWindow.length > 0 && lastMessageTime - rateWindow[0] > RATE_WINDOW_MS) rateWindow.shift();
    if (selectedGroupFilter !== '' && msg.group_name !== selectedGroupFilter) return;
    scene.update(msg);
    cameraViews.update(msg.camera_views ?? []);
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

const scene = createPoseScene(canvasContainer);
const cameraViews = createCameraViews(cameraViewsContainer);

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
    return;
  }
  adminApi.request(AdminMethods.ListCameras, { group_name: group })
    .then((result) => {
      const names = result && result.camera_names ? result.camera_names : [];
      if (!cameraSelect) return;
      const current = cameraSelect.value;
      cameraSelect.innerHTML = '<option value="">(all)</option>' +
        names.map((n) => `<option value="${escapeHtml(n)}"${n === current ? ' selected' : ''}>${escapeHtml(n)}</option>`).join('');
    })
    .catch(() => {
      if (cameraSelect) cameraSelect.innerHTML = '<option value="">(all)</option>';
    });
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

// Group selector: filter pose stream by selected group; refresh camera list when group changes
if (groupSelect) {
  groupSelect.addEventListener('change', () => {
    selectedGroupFilter = groupSelect.value;
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
  command: { start: { with_pose: true, with_image: false, fps: 0 } },
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
