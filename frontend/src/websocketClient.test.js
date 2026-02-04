import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createPoseStreamClient } from './websocketClient.js';

const OPEN = 1;

describe('createPoseStreamClient', () => {
  /** @type {ReturnType<typeof createPoseStreamClient>} */
  let client;
  /** @type {{ readyState: number, send: ReturnType<typeof vi.fn>, close: ReturnType<typeof vi.fn> }} */
  let wsMock;

  beforeEach(() => {
    wsMock = {
      readyState: 0,
      send: vi.fn(),
      close: vi.fn(),
      addEventListener: vi.fn(),
    };
    vi.stubGlobal('WebSocket', vi.fn(function (url) {
      const w = { ...wsMock, url };
      const self = this;
      setTimeout(() => {
        w.readyState = OPEN;
        if (w.onopen) w.onopen();
      }, 0);
      return w;
    }));
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('accepts options object with onPoseMessage and onRawMessage', () => {
    const onPoseMessage = vi.fn();
    const onRawMessage = vi.fn();
    client = createPoseStreamClient('ws://test', { onPoseMessage, onRawMessage });
    expect(client).toHaveProperty('connect');
    expect(client).toHaveProperty('disconnect');
    expect(client).toHaveProperty('getState');
    expect(client).toHaveProperty('send');
    client.connect();
    expect(WebSocket).toHaveBeenCalledWith('ws://test');
  });

  it('accepts legacy callback as second argument (onPoseMessage only)', () => {
    const onMessage = vi.fn();
    client = createPoseStreamClient('ws://test', onMessage);
    client.connect();
    expect(WebSocket).toHaveBeenCalledWith('ws://test');
  });

  it('getState returns closed before connect', () => {
    client = createPoseStreamClient('ws://test', { onPoseMessage: vi.fn() });
    expect(client.getState()).toBe('closed');
  });

  it('send() calls ws.send when socket is open', async () => {
    const sendFn = vi.fn();
    vi.stubGlobal('WebSocket', vi.fn(function () {
      const w = { readyState: 0, send: sendFn, close: vi.fn(), addEventListener: vi.fn() };
      setTimeout(() => {
        w.readyState = 1;
        if (w.onopen) w.onopen();
      }, 0);
      return w;
    }));
    vi.mocked(WebSocket).OPEN = 1;
    client = createPoseStreamClient('ws://test', { onPoseMessage: vi.fn() });
    client.connect();
    await vi.waitFor(() => expect(client.getState()).toBe('open'), { timeout: 100 });
    client.send('hello');
    expect(sendFn).toHaveBeenCalledWith('hello');
  });

  it('when onRawMessage returns true, pose message is not parsed', async () => {
    const onPoseMessage = vi.fn();
    const onRawMessage = vi.fn((raw) => typeof raw === 'string' && raw.startsWith('{"id":'));
    client = createPoseStreamClient('ws://test', { onPoseMessage, onRawMessage });
    client.connect();
    await vi.waitFor(() => expect(client.getState()).toBe('open'), { timeout: 100 });
    const ws = vi.mocked(WebSocket).mock.results[0].value;
    ws.onmessage({ data: '{"id":1,"result":{}}' });
    expect(onRawMessage).toHaveBeenCalledWith('{"id":1,"result":{}}');
    expect(onPoseMessage).not.toHaveBeenCalled();
  });
});
