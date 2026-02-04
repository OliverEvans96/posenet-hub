/**
 * WebSocket client for the hub pose stream. Reconnects with backoff.
 */

import { parsePoseStreamMessage } from './poseMessage.js';

const DEFAULT_WS_URL = 'ws://localhost:9001';

/**
 * @param {string} [url]
 * @param {(data: import('./poseMessage.js').PoseStreamMessage) => void} onMessage
 * @returns {{ connect: () => void, disconnect: () => void, getState: () => 'connecting'|'open'|'closed' }}
 */
export function createPoseStreamClient(url = DEFAULT_WS_URL, onMessage) {
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
      if (typeof event.data === 'string' && onMessage) {
        const parsed = parsePoseStreamMessage(event.data);
        if (parsed) onMessage(parsed);
      }
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

  return { connect, disconnect, getState };
}

