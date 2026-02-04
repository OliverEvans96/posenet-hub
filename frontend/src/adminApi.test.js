import { describe, it, expect } from 'vitest';
import { createAdminApi, AdminMethods } from './adminApi.js';

describe('createAdminApi', () => {
  it('sends request with id, method, and params', async () => {
    const sent = [];
    let responseHandler;
    const api = createAdminApi(
      (msg) => sent.push(msg),
      (h) => { responseHandler = h; }
    );
    const p = api.request('ListGroups', {});
    const msg = JSON.parse(sent[0]);
    expect(msg).toHaveProperty('id');
    expect(msg.method).toBe('ListGroups');
    expect(msg.params).toEqual({});
    responseHandler(JSON.stringify({ id: msg.id, result: { group_names: [] } }));
    await expect(p).resolves.toEqual({ group_names: [] });
  });

  it('resolves with result when handleResponse receives result', async () => {
    const sent = [];
    let responseHandler;
    const api = createAdminApi(
      (msg) => sent.push(msg),
      (h) => { responseHandler = h; }
    );
    const p = api.request('ListCameras', { group_name: 'g1' });
    const msg = JSON.parse(sent[0]);
    responseHandler(JSON.stringify({ id: msg.id, result: { camera_names: ['c1'] } }));
    await expect(p).resolves.toEqual({ camera_names: ['c1'] });
  });

  it('rejects with error when handleResponse receives error', async () => {
    const sent = [];
    let responseHandler;
    const api = createAdminApi(
      (msg) => sent.push(msg),
      (h) => { responseHandler = h; }
    );
    const p = api.request('ListGroups', {});
    const msg = JSON.parse(sent[0]);
    responseHandler(JSON.stringify({ id: msg.id, error: 'Not found' }));
    await expect(p).rejects.toThrow('Not found');
  });

  it('handleResponse returns true when id matches pending request', () => {
    const sent = [];
    let responseHandler;
    const api = createAdminApi(
      (msg) => sent.push(msg),
      (h) => { responseHandler = h; }
    );
    api.request('ListGroups', {});
    const msg = JSON.parse(sent[0]);
    const out = responseHandler(JSON.stringify({ id: msg.id, result: {} }));
    expect(out).toBe(true);
  });

  it('handleResponse returns false for non-JSON', () => {
    let responseHandler;
    createAdminApi(() => {}, (h) => { responseHandler = h; });
    expect(responseHandler('not json')).toBe(false);
  });

  it('handleResponse returns false when id does not match', () => {
    let responseHandler;
    createAdminApi(() => {}, (h) => { responseHandler = h; });
    expect(responseHandler(JSON.stringify({ id: 99999, result: {} }))).toBe(false);
  });

  it('accepts string id in response', async () => {
    const sent = [];
    let responseHandler;
    const api = createAdminApi(
      (msg) => sent.push(msg),
      (h) => { responseHandler = h; }
    );
    const p = api.request('Ping', {});
    const msg = JSON.parse(sent[0]);
    responseHandler(JSON.stringify({ id: String(msg.id), result: { results: [] } }));
    await expect(p).resolves.toEqual({ results: [] });
  });
});

describe('AdminMethods', () => {
  it('exposes all admin method names', () => {
    expect(AdminMethods.ListGroups).toBe('ListGroups');
    expect(AdminMethods.ListCameras).toBe('ListCameras');
    expect(AdminMethods.GetCameraInfo).toBe('GetCameraInfo');
    expect(AdminMethods.StreamControl).toBe('StreamControl');
    expect(AdminMethods.TakeSnapshots).toBe('TakeSnapshots');
    expect(AdminMethods.GetCurrent).toBe('GetCurrent');
    expect(AdminMethods.Calibrate).toBe('Calibrate');
    expect(AdminMethods.Ping).toBe('Ping');
    expect(AdminMethods.UpdateCameras).toBe('UpdateCameras');
  });
});
