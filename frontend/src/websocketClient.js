/**
 * WebSocket client for the hub pose stream and admin API. Reconnects with backoff.
 */

import { parsePoseStreamMessage } from './poseMessage.js';

/** Default WebSocket URL from current location (so mobile can connect when opening via host IP). */
function getDefaultWsUrl() {
  if (typeof window === 'undefined' || !window.location) return 'ws://localhost:9001';
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.hostname || 'localhost';
  const port = new URL(window.location.href).searchParams.get('ws') || '9001';
  return `${proto}//${host}:${port}`;
}

/**
 * @param {string} [url]
 * @param {{ onPoseMessage: (data: import('./poseMessage.js').PoseStreamMessage) => void, onRawMessage?: (raw: string) => boolean }} options
 *   - onPoseMessage: called for each pose stream message
 *   - onRawMessage: optional; called with raw string first. If it returns true, the message is not parsed as pose.
 * @returns {{ connect: () => void, disconnect: () => void, getState: () => 'connecting'|'open'|'closed', send: (text: string) => void }}
 */
export function createPoseStreamClient(url, options) {
  const resolvedUrl = url ?? getDefaultWsUrl();
  const { onPoseMessage, onRawMessage } = typeof options === 'function' ? { onPoseMessage: options, onRawMessage: undefined } : options;

  /** @type {WebSocket|null} */
  let ws = null;
  /** @type {'connecting'|'open'|'closed'} */
  let state = 'closed';
  let reconnectAttempts = 0;
  const maxReconnectDelay = 10000;
  let reconnectTimer = null;

  function getState() {
    return state;
  }

  function send(text) {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(text);
    }
  }

  function disconnect() {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (ws) {
      ws.close();
      ws = null;
    }
    state = 'closed';
  }

  function connect() {
    if (ws && (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN)) {
      return;
    }
    state = 'connecting';
    ws = new WebSocket(resolvedUrl);
    ws.onopen = () => {
      state = 'open';
      reconnectAttempts = 0;
    };
    ws.onmessage = (event) => {
      if (typeof event.data !== 'string') return;
      const raw = event.data;
      if (onRawMessage && onRawMessage(raw)) return;
      const parsed = parsePoseStreamMessage(raw);
      if (parsed && onPoseMessage) onPoseMessage(parsed);
    };
    ws.onclose = () => {
      state = 'closed';
      ws = null;
      const delay = Math.min(1000 * 2 ** reconnectAttempts, maxReconnectDelay);
      reconnectAttempts++;
      reconnectTimer = setTimeout(connect, delay);
    };
    ws.onerror = () => {};
  }

  return { connect, disconnect, getState, send };
}
