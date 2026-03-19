export interface Clip {
  id: string;
  content_type: 'text' | 'image' | 'file_ref';
  text_content: string | null;
  image_hash: string | null;
  image_mime: string | null;
  source_device: string;
  source_app: string | null;
  byte_size: number;
  created_at: string;
  is_favorite: boolean;
  is_deleted: boolean;
}

export interface ClipListResponse {
  clips: Clip[];
  total_count: number;
}

export interface TauriApiResponse {
  status: number;
  body: string;
  content_type?: string;
}

export interface AppSettings {
  server_url: string;
  api_key: string;
  insecure: boolean;
  sync_enabled?: boolean;
}

export type TauriInvoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

declare global {
  interface Window {
    __TAURI_INTERNALS__?: {
      invoke: TauriInvoke;
    };
  }
}
