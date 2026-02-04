/**
 * Entry: connect to hub WebSocket, visualize poses, show info.
 */

import { createPoseStreamClient } from './websocketClient.js';
import { createPoseScene } from './scene.js';

const wsUrl = getWsUrl();
const infoBar = document.getElementById('info-bar');
const connectionStatus = document.getElementById('connection-status');
const groupNameEl = document.getElementById('group-name');
const poseCountEl = document.getElementById('pose-count');
const timestampEl = document.getElementById('timestamp');
const messageRateEl = document.getElementById('message-rate');
const canvasContainer = document.getElementById('canvas-container');

let lastMessageTime = 0;
const rateWindow = [];
const RATE_WINDOW_MS = 2000;

function getWsUrl() {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.hostname || 'localhost';
  const wsPort = new URL(window.location.href).searchParams.get('ws') || '9001';
  return `${proto}//${host}:${wsPort}`;
}

const client = createPoseStreamClient(wsUrl, (msg) => {
  lastMessageTime = Date.now();
  rateWindow.push(lastMessageTime);
  while (rateWindow.length > 0 && lastMessageTime - rateWindow[0] > RATE_WINDOW_MS) rateWindow.shift();
  scene.update(msg);
  updateInfo(msg);
});

const scene = createPoseScene(canvasContainer);

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

setInterval(() => {
  updateConnectionStatus();
  while (rateWindow.length > 0 && Date.now() - rateWindow[0] > RATE_WINDOW_MS) rateWindow.shift();
  const rate = rateWindow.length / (RATE_WINDOW_MS / 1000);
  messageRateEl.textContent = `${rate.toFixed(1)} msg/s`;
}, 300);
client.connect();
updateConnectionStatus();

function animate() {
  requestAnimationFrame(animate);
  scene.render();
}
animate();
