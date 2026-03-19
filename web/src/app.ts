import type { Clip, ClipListResponse, AppSettings } from './types';
import { createBrowserApi, createTauriApi, type ApiClient } from './api';
import { relativeTime } from './time';
import { initToast, showToast } from './toast';

const IS_TAURI = window.__TAURI_INTERNALS__ !== undefined;
const invoke = IS_TAURI ? window.__TAURI_INTERNALS__!.invoke : null;

let api: ApiClient = createBrowserApi('/api/v1');
if (IS_TAURI && invoke) {
  api = createTauriApi(invoke);
}

const clipsEl = document.getElementById('clips')!;
const searchEl = document.getElementById('search') as HTMLInputElement;
const emptyEl = document.getElementById('empty-state')!;
const clipCountEl = document.getElementById('clip-count')!;
const deleteModal = document.getElementById('delete-modal')!;
const modalCancel = document.getElementById('modal-cancel')!;
const modalConfirm = document.getElementById('modal-confirm')!;

initToast(document.getElementById('toast-area')!);

let activeFilter = 'all';
let pendingDeleteId: string | null = null;
let lastClipIds = '';

// ── Filters ──────────────────────────────────────────

document.getElementById('filters')!.addEventListener('click', (e) => {
  const btn = (e.target as HTMLElement).closest('.filter-btn') as HTMLElement | null;
  if (!btn) return;
  document.querySelectorAll('.filter-btn').forEach(b => b.classList.remove('active'));
  btn.classList.add('active');
  activeFilter = btn.dataset.filter ?? 'all';
  loadClips();
});

// ── Search ───────────────────────────────────────────

let debounceTimer: ReturnType<typeof setTimeout>;
searchEl.addEventListener('input', () => {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(loadClips, 250);
});

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    if (!deleteModal.hidden) {
      closeModal();
      return;
    }
    if (searchEl.value) {
      searchEl.value = '';
      searchEl.focus();
      loadClips();
    }
  }
});

// ── Event delegation ─────────────────────────────────

clipsEl.addEventListener('click', (e) => {
  const target = e.target as HTMLElement;

  const deleteBtn = target.closest('.clip-delete');
  if (deleteBtn) {
    e.stopPropagation();
    const clipEl = deleteBtn.closest('.clip') as HTMLElement | null;
    if (clipEl?.dataset.id) confirmDelete(clipEl.dataset.id);
    return;
  }

  const favBtn = target.closest('.fav') as HTMLElement | null;
  if (favBtn) {
    e.stopPropagation();
    const clipEl = favBtn.closest('.clip') as HTMLElement | null;
    if (clipEl?.dataset.id) toggleFav(clipEl.dataset.id, favBtn);
    return;
  }

  const clipEl = target.closest('.clip') as HTMLElement | null;
  if (clipEl) copyClip(clipEl);
});

// ── Load & render ────────────────────────────────────

async function loadClips(): Promise<void> {
  const search = searchEl.value.trim();
  let qs = '?limit=50';
  if (search) qs += `&search=${encodeURIComponent(search)}`;
  if (activeFilter !== 'all') qs += `&content_type=${activeFilter}`;

  try {
    const data = await api.get<ClipListResponse>(`/clips${qs}`);
    const clips = data.clips ?? [];
    const newIds = clips.map(c => c.id).join(',');

    if (newIds === lastClipIds && clipsEl.children.length > 0) return;
    lastClipIds = newIds;

    updateCount(clips.length);
    render(clips);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    console.error('Failed to load clips:', e);
    showToast(`Failed to load clips: ${msg}`);
  }
}

function updateCount(n: number): void {
  clipCountEl.textContent = n > 0 ? `${n} clip${n !== 1 ? 's' : ''}` : '';
}

function render(clips: Clip[]): void {
  if (clips.length === 0) {
    clipsEl.innerHTML = '';
    emptyEl.hidden = false;
    return;
  }

  emptyEl.hidden = true;
  clipsEl.innerHTML = '';

  clips.forEach((clip, i) => {
    const el = document.createElement('div');
    el.className = 'clip';
    el.dataset.id = clip.id;
    el.dataset.type = clip.content_type;
    el.style.animationDelay = `${i * 0.03}s`;

    const deleteBtn = document.createElement('button');
    deleteBtn.className = 'clip-delete';
    deleteBtn.title = 'Delete clip';
    deleteBtn.innerHTML = '&times;';
    el.appendChild(deleteBtn);

    const headerDiv = document.createElement('div');
    headerDiv.className = 'clip-header';

    const badge = document.createElement('span');
    badge.className = `badge ${clip.content_type === 'text' ? 'badge-text' : 'badge-image'}`;
    badge.textContent = clip.content_type === 'text' ? 'Text' : 'Image';
    headerDiv.appendChild(badge);

    if (clip.source_device) {
      const deviceSpan = document.createElement('span');
      deviceSpan.textContent = clip.source_device;
      headerDiv.appendChild(deviceSpan);
    }

    const timeSpan = document.createElement('span');
    timeSpan.textContent = relativeTime(clip.created_at);
    headerDiv.appendChild(timeSpan);

    const spacer = document.createElement('span');
    spacer.className = 'spacer';
    headerDiv.appendChild(spacer);

    const favSpan = document.createElement('span');
    favSpan.className = `fav${clip.is_favorite ? ' is-fav' : ''}`;
    favSpan.innerHTML = clip.is_favorite ? '&#9733;' : '&#9734;';
    headerDiv.appendChild(favSpan);

    el.appendChild(headerDiv);

    if (clip.content_type === 'text') {
      const textDiv = document.createElement('div');
      textDiv.className = 'clip-text';
      textDiv.textContent = (clip.text_content ?? '').slice(0, 500);
      el.appendChild(textDiv);
    } else if (clip.content_type === 'image') {
      const imgDiv = document.createElement('div');
      imgDiv.className = 'clip-image';
      const img = document.createElement('img');
      img.loading = 'lazy';
      img.alt = 'Clip image';
      if (IS_TAURI && invoke) {
        loadImageProxy(img, clip.id);
      } else {
        img.src = `/api/v1/clips/${encodeURIComponent(clip.id)}/content`;
      }
      imgDiv.appendChild(img);
      el.appendChild(imgDiv);
    }

    clipsEl.appendChild(el);
  });
}

async function loadImageProxy(img: HTMLImageElement, clipId: string): Promise<void> {
  try {
    const b64 = await api.getBytes(`/clips/${clipId}/content`);
    img.src = `data:image/png;base64,${b64}`;
  } catch (e) {
    console.error('Failed to load image:', e);
  }
}

// ── Copy ─────────────────────────────────────────────

async function copyClip(el: HTMLElement): Promise<void> {
  const id = el.dataset.id;
  const type = el.dataset.type;
  if (!id) return;

  try {
    if (type === 'text') {
      const text = await api.getText(`/clips/${encodeURIComponent(id)}/content`);
      if (IS_TAURI && invoke) {
        await invoke('copy_to_clipboard', { text });
      } else {
        await navigator.clipboard.writeText(text);
      }
    } else if (type === 'image') {
      if (IS_TAURI && invoke) {
        const b64 = await api.getBytes(`/clips/${id}/content`);
        const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
        await invoke('copy_image_to_clipboard', { pngData: Array.from(bytes) });
      } else {
        const resp = await fetch(`/api/v1/clips/${encodeURIComponent(id)}/content`);
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        const blob = await resp.blob();
        await navigator.clipboard.write([new ClipboardItem({ [blob.type]: blob })]);
      }
    }
    el.classList.add('copied');
    setTimeout(() => el.classList.remove('copied'), 800);
    showToast('Copied to clipboard');
  } catch (e) {
    console.error('Copy failed:', e);
    showToast('Failed to copy');
  }
}

// ── Favorite ─────────────────────────────────────────

async function toggleFav(id: string, el: HTMLElement): Promise<void> {
  try {
    const data = await api.patch<{ is_favorite: boolean }>(`/clips/${encodeURIComponent(id)}/favorite`);
    el.innerHTML = data.is_favorite ? '&#9733;' : '&#9734;';
    el.classList.toggle('is-fav', data.is_favorite);
  } catch (e) {
    console.error('Favorite toggle failed:', e);
    showToast('Failed to update favorite');
  }
}

// ── Delete ───────────────────────────────────────────

function confirmDelete(id: string): void {
  pendingDeleteId = id;
  deleteModal.hidden = false;
}

function closeModal(): void {
  deleteModal.hidden = true;
  pendingDeleteId = null;
}

modalCancel.addEventListener('click', closeModal);

deleteModal.addEventListener('click', (e) => {
  if (e.target === deleteModal) closeModal();
});

modalConfirm.addEventListener('click', async () => {
  if (!pendingDeleteId) return;
  const id = pendingDeleteId;
  closeModal();
  try {
    await api.delete(`/clips/${encodeURIComponent(id)}`);
    const el = clipsEl.querySelector(`.clip[data-id="${CSS.escape(id)}"]`) as HTMLElement | null;
    if (el) {
      el.style.transition = 'opacity 0.2s, transform 0.2s';
      el.style.opacity = '0';
      el.style.transform = 'translateX(20px)';
      setTimeout(() => { el.remove(); updateCount(clipsEl.children.length); }, 200);
    }
    lastClipIds = '';
    showToast('Clip deleted');
  } catch (e) {
    console.error('Delete failed:', e);
    showToast('Failed to delete clip');
  }
});

// ── Settings (Tauri only) ────────────────────────────

const settingsBtn = document.getElementById('settings-btn');
const settingsModal = document.getElementById('settings-modal');
const settingsCancel = document.getElementById('settings-cancel');
const settingsSave = document.getElementById('settings-save');
const settingServerUrl = document.getElementById('setting-server-url') as HTMLInputElement | null;
const settingApiKey = document.getElementById('setting-api-key') as HTMLInputElement | null;
const settingInsecure = document.getElementById('setting-insecure') as HTMLInputElement | null;

if (IS_TAURI && invoke && settingsBtn && settingsModal && settingsCancel && settingsSave) {
  settingsBtn.hidden = false;

  settingsBtn.addEventListener('click', async () => {
    try {
      const cfg = (await invoke('get_settings')) as AppSettings;
      if (settingServerUrl) settingServerUrl.value = cfg.server_url ?? '';
      if (settingApiKey) settingApiKey.value = cfg.api_key ?? '';
      if (settingInsecure) settingInsecure.checked = cfg.insecure ?? false;
    } catch (e) {
      console.warn('Failed to load settings:', e);
    }
    settingsModal!.hidden = false;
  });

  settingsCancel.addEventListener('click', () => { settingsModal!.hidden = true; });

  settingsModal.addEventListener('click', (e) => {
    if (e.target === settingsModal) settingsModal!.hidden = true;
  });

  settingsSave.addEventListener('click', async () => {
    const cfg = {
      server_url: settingServerUrl?.value.trim() ?? '',
      api_key: settingApiKey?.value ?? '',
      insecure: settingInsecure?.checked ?? false,
    };
    try {
      await invoke!('save_settings', { settings: cfg });
      settingsModal!.hidden = true;
      lastClipIds = '';
      loadClips();
      showToast('Settings saved');
    } catch (e) {
      console.error('Failed to save settings:', e);
      showToast('Failed to save settings');
    }
  });
}

// ── Init ─────────────────────────────────────────────

loadClips();
setInterval(loadClips, 3000);
