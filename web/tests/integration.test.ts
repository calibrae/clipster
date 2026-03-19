import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { createServer, type Server, type IncomingMessage, type ServerResponse } from 'http';
import { createBrowserApi, createTauriApi, type ApiClient } from '../src/api';
import type { TauriApiResponse, TauriInvoke } from '../src/types';

// ── Test HTTP server that mimics the clipster API ─────

interface StoredClip {
  id: string;
  content_type: string;
  text_content: string | null;
  source_device: string;
  created_at: string;
  is_favorite: boolean;
  is_deleted: boolean;
  byte_size: number;
  content_hash: string;
}

let server: Server;
let baseUrl: string;
const clips: Map<string, StoredClip> = new Map();
let nextId = 1;

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve) => {
    let body = '';
    req.on('data', (chunk: Buffer) => { body += chunk.toString(); });
    req.on('end', () => resolve(body));
  });
}

function handleRequest(req: IncomingMessage, res: ServerResponse): void {
  const url = new URL(req.url ?? '/', `http://localhost`);
  const path = url.pathname;
  const method = req.method ?? 'GET';

  res.setHeader('Content-Type', 'application/json');

  // Health
  if (path === '/api/v1/health' && method === 'GET') {
    res.end(JSON.stringify({ status: 'ok' }));
    return;
  }

  // List clips
  if (path === '/api/v1/clips' && method === 'GET') {
    const search = url.searchParams.get('search');
    const limit = parseInt(url.searchParams.get('limit') ?? '50');
    const offset = parseInt(url.searchParams.get('offset') ?? '0');
    const contentType = url.searchParams.get('content_type');

    let results = [...clips.values()].filter(c => !c.is_deleted);
    if (search) results = results.filter(c => c.text_content?.includes(search));
    if (contentType) results = results.filter(c => c.content_type === contentType);
    const total = results.length;
    results = results.slice(offset, offset + limit);

    res.end(JSON.stringify({ clips: results, total_count: total }));
    return;
  }

  // Create clip
  if (path === '/api/v1/clips' && method === 'POST') {
    readBody(req).then((body) => {
      const data = JSON.parse(body);
      const id = `clip-${nextId++}`;
      const clip: StoredClip = {
        id,
        content_type: 'text',
        text_content: data.text_content,
        source_device: data.source_device ?? 'test',
        created_at: new Date().toISOString(),
        is_favorite: false,
        is_deleted: false,
        byte_size: (data.text_content ?? '').length,
        content_hash: `hash-${id}`,
      };
      clips.set(id, clip);
      res.writeHead(201);
      res.end(JSON.stringify(clip));
    });
    return;
  }

  // Get clip by ID
  const clipMatch = path.match(/^\/api\/v1\/clips\/([^/]+)$/);
  if (clipMatch && method === 'GET') {
    const clip = clips.get(clipMatch[1]!);
    if (!clip || clip.is_deleted) {
      res.writeHead(404);
      res.end(JSON.stringify({ error: 'not found' }));
    } else {
      res.end(JSON.stringify(clip));
    }
    return;
  }

  // Get clip content
  const contentMatch = path.match(/^\/api\/v1\/clips\/([^/]+)\/content$/);
  if (contentMatch && method === 'GET') {
    const clip = clips.get(contentMatch[1]!);
    if (!clip || clip.is_deleted) {
      res.writeHead(404);
      res.end(JSON.stringify({ error: 'not found' }));
    } else {
      res.setHeader('Content-Type', 'text/plain');
      res.end(clip.text_content ?? '');
    }
    return;
  }

  // Delete clip
  const deleteMatch = path.match(/^\/api\/v1\/clips\/([^/]+)$/);
  if (deleteMatch && method === 'DELETE') {
    const clip = clips.get(deleteMatch[1]!);
    if (!clip) {
      res.writeHead(404);
      res.end(JSON.stringify({ error: 'not found' }));
    } else {
      clip.is_deleted = true;
      res.writeHead(204);
      res.end();
    }
    return;
  }

  // Toggle favorite
  const favMatch = path.match(/^\/api\/v1\/clips\/([^/]+)\/favorite$/);
  if (favMatch && method === 'PATCH') {
    const clip = clips.get(favMatch[1]!);
    if (!clip || clip.is_deleted) {
      res.writeHead(404);
      res.end(JSON.stringify({ error: 'not found' }));
    } else {
      clip.is_favorite = !clip.is_favorite;
      res.end(JSON.stringify({ is_favorite: clip.is_favorite }));
    }
    return;
  }

  res.writeHead(404);
  res.end(JSON.stringify({ error: 'not found' }));
}

beforeAll(async () => {
  server = createServer(handleRequest);
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      if (addr && typeof addr === 'object') {
        baseUrl = `http://127.0.0.1:${addr.port}`;
      }
      resolve();
    });
  });
});

afterAll(() => {
  server.close();
});

// ── Helper: create a Tauri-like invoke that proxies through HTTP ──

function createTestTauriInvoke(serverUrl: string): TauriInvoke {
  return async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'api_request') {
      const req = args?.req as { method: string; path: string; body?: string };
      const url = `${serverUrl}/api/v1${req.path}`;
      const init: RequestInit = { method: req.method };
      if (req.body) {
        init.headers = { 'content-type': 'application/json' };
        init.body = req.body;
      }
      const resp = await fetch(url, init);
      const body = await resp.text();
      return { status: resp.status, body } satisfies TauriApiResponse;
    }
    if (cmd === 'api_fetch_bytes') {
      const path = args?.path as string;
      const resp = await fetch(`${serverUrl}/api/v1${path}`);
      const buf = await resp.arrayBuffer();
      return Buffer.from(buf).toString('base64');
    }
    throw new Error(`unknown cmd: ${cmd}`);
  };
}

// ── Tests for browser API client ─────────────────────

describe('Browser API integration', () => {
  let api: ApiClient;

  beforeAll(() => {
    api = createBrowserApi(`${baseUrl}/api/v1`);
    clips.clear();
    nextId = 1;
  });

  it('health check', async () => {
    const data = await api.get<{ status: string }>('/health');
    expect(data.status).toBe('ok');
  });

  it('creates a text clip', async () => {
    const clip = await api.post<StoredClip>('/clips', {
      text_content: 'hello from browser test',
      source_device: 'test-browser',
    });
    expect(clip.id).toBe('clip-1');
    expect(clip.content_type).toBe('text');
    expect(clip.text_content).toBe('hello from browser test');
  });

  it('lists clips', async () => {
    const data = await api.get<{ clips: StoredClip[]; total_count: number }>('/clips?limit=50');
    expect(data.total_count).toBe(1);
    expect(data.clips).toHaveLength(1);
    expect(data.clips[0]!.text_content).toBe('hello from browser test');
  });

  it('gets clip content as text', async () => {
    const text = await api.getText('/clips/clip-1/content');
    expect(text).toBe('hello from browser test');
  });

  it('toggles favorite', async () => {
    const result = await api.patch<{ is_favorite: boolean }>('/clips/clip-1/favorite');
    expect(result.is_favorite).toBe(true);

    const result2 = await api.patch<{ is_favorite: boolean }>('/clips/clip-1/favorite');
    expect(result2.is_favorite).toBe(false);
  });

  it('deletes a clip', async () => {
    await api.delete('/clips/clip-1');
    const data = await api.get<{ clips: StoredClip[]; total_count: number }>('/clips?limit=50');
    expect(data.total_count).toBe(0);
  });

  it('returns 404 for deleted clip', async () => {
    await expect(api.get('/clips/clip-1')).rejects.toThrow('HTTP 404');
  });

  it('search filter works', async () => {
    await api.post('/clips', { text_content: 'rust programming' });
    await api.post('/clips', { text_content: 'python scripting' });

    const data = await api.get<{ clips: StoredClip[]; total_count: number }>('/clips?search=rust');
    expect(data.total_count).toBe(1);
    expect(data.clips[0]!.text_content).toBe('rust programming');
  });

  it('pagination works', async () => {
    const page1 = await api.get<{ clips: StoredClip[]; total_count: number }>('/clips?limit=1&offset=0');
    const page2 = await api.get<{ clips: StoredClip[]; total_count: number }>('/clips?limit=1&offset=1');

    expect(page1.clips).toHaveLength(1);
    expect(page2.clips).toHaveLength(1);
    expect(page1.clips[0]!.id).not.toBe(page2.clips[0]!.id);
  });
});

// ── Tests for Tauri API client (simulated) ───────────

describe('Tauri API integration', () => {
  let api: ApiClient;

  beforeAll(() => {
    const invoke = createTestTauriInvoke(baseUrl);
    api = createTauriApi(invoke);
    clips.clear();
    nextId = 100;
  });

  it('creates a clip through Tauri proxy', async () => {
    const clip = await api.post<StoredClip>('/clips', {
      text_content: 'hello from tauri test',
      source_device: 'test-tauri',
    });
    expect(clip.id).toBe('clip-100');
    expect(clip.text_content).toBe('hello from tauri test');
  });

  it('lists clips through Tauri proxy', async () => {
    const data = await api.get<{ clips: StoredClip[]; total_count: number }>('/clips?limit=50');
    expect(data.total_count).toBe(1);
  });

  it('gets text content through Tauri proxy', async () => {
    const text = await api.getText('/clips/clip-100/content');
    expect(text).toBe('hello from tauri test');
  });

  it('gets bytes through Tauri proxy', async () => {
    const b64 = await api.getBytes('/clips/clip-100/content');
    const decoded = Buffer.from(b64, 'base64').toString();
    expect(decoded).toBe('hello from tauri test');
  });

  it('deletes through Tauri proxy', async () => {
    await api.delete('/clips/clip-100');
    const data = await api.get<{ clips: StoredClip[]; total_count: number }>('/clips?limit=50');
    expect(data.total_count).toBe(0);
  });

  it('favorite toggle through Tauri proxy', async () => {
    const clip = await api.post<StoredClip>('/clips', { text_content: 'fav test' });
    const result = await api.patch<{ is_favorite: boolean }>(`/clips/${clip.id}/favorite`);
    expect(result.is_favorite).toBe(true);
  });
});
