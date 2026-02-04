/**
 * Admin API over WebSocket (JSON-RPC style).
 * Request: { id, method, params? }
 * Response: { id, result? } | { id, error? }
 */

let nextId = 1;
/** @type {Map<number, { resolve: (v: unknown) => void, reject: (e: Error) => void }>} */
const pending = new Map();

/**
 * @param {(msg: string) => void} send
 * @param {(raw: string) => void} registerResponseHandler - call with (raw) so we can handle admin responses
 * @returns {{ request: (method: string, params?: object) => Promise<unknown>, handleResponse: (raw: string) => boolean }}
 */
export function createAdminApi(send, registerResponseHandler) {
  function handleResponse(raw) {
    try {
      const data = JSON.parse(raw);
      if (typeof data.id !== 'number' && typeof data.id !== 'string') return false;
      const id = data.id;
      const key = String(id);
      const entry = pending.get(key);
      if (!entry) return false;
      pending.delete(key);
      if ('error' in data && data.error != null) {
        entry.reject(new Error(String(data.error)));
      } else if ('result' in data) {
        entry.resolve(data.result);
      } else {
        entry.reject(new Error('Invalid admin response'));
      }
      return true;
    } catch {
      return false;
    }
  }

  registerResponseHandler((raw) => handleResponse(raw));

  /**
   * @param {string} method
   * @param {object} [params]
   * @returns {Promise<unknown>}
   */
  function request(method, params) {
    return new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(String(id), { resolve, reject });
      const msg = JSON.stringify({ id, method, params: params ?? {} });
      send(msg);
    });
  }

  return { request, handleResponse };
}

/** Admin method names matching server. */
export const AdminMethods = {
  ListGroups: 'ListGroups',
  ListCameras: 'ListCameras',
  GetCameraInfo: 'GetCameraInfo',
  StreamControl: 'StreamControl',
  TakeSnapshots: 'TakeSnapshots',
  GetCurrent: 'GetCurrent',
  Calibrate: 'Calibrate',
  Ping: 'Ping',
  UpdateCameras: 'UpdateCameras',
};
