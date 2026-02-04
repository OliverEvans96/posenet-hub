/**
 * WebSocket client for the hub pose stream and admin API. Reconnects with backoff.
 */

import { parsePoseStreamMessage } from './poseMessage.js';

const DEFAULT_WS_URL = 'ws://localhost:9001';

/**
 * @param {string} [url]
 * @param {{ onPoseMessage: (data: import('./poseMessage.js').PoseStreamMessage) => void, onRawMessage?: (raw: string) => boolean }} options
 *   - onPoseMessage: called for each pose stream message
 *   - onRawMessage: optional; called with raw string first. If it returns true, the message is not parsed as pose.
 * @returns {{ connect: () => void, disconnect: () => void, getState: () => 'connecting'|'open'|'closed', send: (text: string) => void }}
 */
export function createPoseStreamClient(url = DEFAULT_WS_URL, options) {
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
    ws = new WebSocket(url);
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
