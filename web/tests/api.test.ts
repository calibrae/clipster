import { describe, it, expect, vi } from 'vitest';
import { buildApiPath, createBrowserApi, createTauriApi } from '../src/api';
import type { TauriApiResponse, TauriInvoke } from '../src/types';

describe('buildApiPath', () => {
  it('concatenates base and path', () => {
    expect(buildApiPath('/api/v1', '/clips')).toBe('/api/v1/clips');
  });

  it('handles query strings', () => {
    expect(buildApiPath('/api/v1', '/clips?limit=50')).toBe('/api/v1/clips?limit=50');
  });

  it('does not double-prefix', () => {
    const result = buildApiPath('/api/v1', '/clips');
    expect(result).not.toContain('/api/v1/api/v1');
  });

  it('handles empty base', () => {
    expect(buildApiPath('', '/clips')).toBe('/clips');
  });
});

describe('createBrowserApi', () => {
  it('constructs correct URLs for GET', async () => {
    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({ clips: [], total_count: 0 }),
    });
    vi.stubGlobal('fetch', mockFetch);

    const api = createBrowserApi('/api/v1');
    await api.get('/clips?limit=50');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/clips?limit=50');

    vi.unstubAllGlobals();
  });

  it('throws on non-ok response', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: false,
      status: 500,
    }));

    const api = createBrowserApi('/api/v1');
    await expect(api.get('/clips')).rejects.toThrow('HTTP 500');

    vi.unstubAllGlobals();
  });

  it('constructs correct URL for DELETE', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal('fetch', mockFetch);

    const api = createBrowserApi('/api/v1');
    await api.delete('/clips/abc-123');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/clips/abc-123', { method: 'DELETE' });

    vi.unstubAllGlobals();
  });

  it('sends JSON body for POST', async () => {
    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({ id: '123' }),
    });
    vi.stubGlobal('fetch', mockFetch);

    const api = createBrowserApi('/api/v1');
    await api.post('/clips', { text_content: 'hello' });

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/clips', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{"text_content":"hello"}',
    });

    vi.unstubAllGlobals();
  });
});

describe('createTauriApi', () => {
  function mockInvoke(responses: Record<string, unknown>): TauriInvoke {
    return async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === 'api_request') {
        const req = args?.req as { method: string; path: string };
        const key = `${req.method} ${req.path}`;
        const body = responses[key] ?? responses['*'];
        return {
          status: 200,
          body: JSON.stringify(body),
        } satisfies TauriApiResponse;
      }
      if (cmd === 'api_fetch_bytes') {
        return responses['bytes'] ?? 'base64data';
      }
      throw new Error(`unknown cmd: ${cmd}`);
    };
  }

  it('sends correct method and path for GET', async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    const invoke: TauriInvoke = async (cmd, args) => {
      calls.push({ cmd, args });
      return { status: 200, body: '{"clips":[],"total_count":0}' };
    };

    const api = createTauriApi(invoke);
    await api.get('/clips?limit=50');

    expect(calls[0]!.cmd).toBe('api_request');
    const req = (calls[0]!.args as Record<string, unknown>).req as { method: string; path: string };
    expect(req.method).toBe('GET');
    expect(req.path).toBe('/clips?limit=50');
  });

  it('throws on 4xx response', async () => {
    const invoke: TauriInvoke = async () => ({
      status: 401,
      body: '{"error":"unauthorized"}',
    });

    const api = createTauriApi(invoke);
    await expect(api.get('/clips')).rejects.toThrow('HTTP 401');
  });

  it('calls api_fetch_bytes for getBytes', async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    const invoke: TauriInvoke = async (cmd, args) => {
      calls.push({ cmd, args });
      return 'aGVsbG8='; // base64 "hello"
    };

    const api = createTauriApi(invoke);
    const result = await api.getBytes('/clips/123/content');

    expect(calls[0]!.cmd).toBe('api_fetch_bytes');
    expect((calls[0]!.args as Record<string, unknown>).path).toBe('/clips/123/content');
    expect(result).toBe('aGVsbG8=');
  });

  it('sends body for POST', async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    const invoke: TauriInvoke = async (cmd, args) => {
      calls.push({ cmd, args });
      return { status: 201, body: '{"id":"123"}' };
    };

    const api = createTauriApi(invoke);
    await api.post('/clips', { text_content: 'hello' });

    const req = (calls[0]!.args as Record<string, unknown>).req as Record<string, unknown>;
    expect(req.body).toBe('{"text_content":"hello"}');
  });
});
