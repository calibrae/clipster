import type { TauriApiResponse, TauriInvoke } from './types';

export interface ApiClient {
  get: <T = unknown>(path: string) => Promise<T>;
  getText: (path: string) => Promise<string>;
  getBytes: (path: string) => Promise<string>;
  post: <T = unknown>(path: string, body: unknown) => Promise<T>;
  delete: (path: string) => Promise<void>;
  patch: <T = unknown>(path: string) => Promise<T>;
}

export function buildApiPath(base: string, path: string): string {
  return `${base}${path}`;
}

export function createBrowserApi(apiBase: string): ApiClient {
  return {
    async get<T>(path: string): Promise<T> {
      const resp = await fetch(buildApiPath(apiBase, path));
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      return resp.json();
    },

    async getText(path: string): Promise<string> {
      const resp = await fetch(buildApiPath(apiBase, path));
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      return resp.text();
    },

    async getBytes(path: string): Promise<string> {
      const resp = await fetch(buildApiPath(apiBase, path));
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const buf = await resp.arrayBuffer();
      return btoa(String.fromCharCode(...new Uint8Array(buf)));
    },

    async post<T>(path: string, body: unknown): Promise<T> {
      const resp = await fetch(buildApiPath(apiBase, path), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      return resp.json();
    },

    async delete(path: string): Promise<void> {
      const resp = await fetch(buildApiPath(apiBase, path), { method: 'DELETE' });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    },

    async patch<T>(path: string): Promise<T> {
      const resp = await fetch(buildApiPath(apiBase, path), { method: 'PATCH' });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      return resp.json();
    },
  };
}

export function createTauriApi(invoke: TauriInvoke): ApiClient {
  async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const req: Record<string, unknown> = { method, path };
    if (body !== undefined) req.body = JSON.stringify(body);
    const res = (await invoke('api_request', { req })) as TauriApiResponse;
    if (res.status >= 400) throw new Error(`HTTP ${res.status}: ${res.body}`);
    return JSON.parse(res.body) as T;
  }

  return {
    get: <T>(path: string) => request<T>('GET', path),

    async getText(path: string): Promise<string> {
      const res = (await invoke('api_request', { req: { method: 'GET', path } })) as TauriApiResponse;
      if (res.status >= 400) throw new Error(`HTTP ${res.status}`);
      return res.body;
    },

    async getBytes(path: string): Promise<string> {
      return (await invoke('api_fetch_bytes', { path })) as string;
    },

    post: <T>(path: string, body: unknown) => request<T>('POST', path, body),

    async delete(path: string): Promise<void> {
      const res = (await invoke('api_request', { req: { method: 'DELETE', path } })) as TauriApiResponse;
      if (res.status >= 400) throw new Error(`HTTP ${res.status}: ${res.body}`);
    },

    patch: <T>(path: string) => request<T>('PATCH', path),
  };
}
