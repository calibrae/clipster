const IS_TAURI = window.__TAURI_INTERNALS__ !== undefined;
const invoke = IS_TAURI ? window.__TAURI_INTERNALS__.invoke : null;
let API = '/api/v1';

const clipsEl = document.getElementById('clips');
const searchEl = document.getElementById('search');
const emptyEl = document.getElementById('empty-state');
const clipCountEl = document.getElementById('clip-count');
const toastArea = document.getElementById('toast-area');
const deleteModal = document.getElementById('delete-modal');
const modalCancel = document.getElementById('modal-cancel');
const modalConfirm = document.getElementById('modal-confirm');

let activeFilter = 'all';
let pendingDeleteId = null;
let lastClipIds = '';

// ── API layer ────────────────────────────────────────
// In Tauri: all requests go through Rust (handles TLS, auth)
// In browser: direct fetch to server

async function apiGet(path) {
  if (IS_TAURI) {
    const res = await invoke('api_request', { req: { method: 'GET', path } });
    if (res.status >= 400) throw new Error(`HTTP ${res.status}: ${res.body}`);
    return JSON.parse(res.body);
  }
  const resp = await fetch(`${API}${path}`);
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  return resp.json();
}

async function apiGetText(path) {
  if (IS_TAURI) {
    const res = await invoke('api_request', { req: { method: 'GET', path } });
    if (res.status >= 400) throw new Error(`HTTP ${res.status}`);
    return res.body;
  }
  const resp = await fetch(`${API}${path}`);
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  return resp.text();
}

async function apiGetBytes(path) {
  if (IS_TAURI) {
    return await invoke('api_fetch_bytes', { path });
  }
  const resp = await fetch(`${API}${path}`);
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  const buf = await resp.arrayBuffer();
  return btoa(String.fromCharCode(...new Uint8Array(buf)));
}

async function apiPost(path, body) {
  if (IS_TAURI) {
    const res = await invoke('api_request', { req: { method: 'POST', path, body: JSON.stringify(body) } });
    if (res.status >= 400) throw new Error(`HTTP ${res.status}: ${res.body}`);
    return JSON.parse(res.body);
  }
  const resp = await fetch(`${API}${path}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  return resp.json();
}

async function apiDelete(path) {
  if (IS_TAURI) {
    const res = await invoke('api_request', { req: { method: 'DELETE', path } });
    if (res.status >= 400) throw new Error(`HTTP ${res.status}: ${res.body}`);
    return;
  }
  const resp = await fetch(`${API}${path}`, { method: 'DELETE' });
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
}

async function apiPatch(path) {
  if (IS_TAURI) {
    const res = await invoke('api_request', { req: { method: 'PATCH', path } });
    if (res.status >= 400) throw new Error(`HTTP ${res.status}: ${res.body}`);
    return JSON.parse(res.body);
  }
  const resp = await fetch(`${API}${path}`, { method: 'PATCH' });
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  return resp.json();
}

function imageUrl(clipId) {
  if (IS_TAURI) {
    // Images loaded via api_fetch_bytes, set in render
    return '';
  }
  return `${API}/clips/${encodeURIComponent(clipId)}/content`;
}

// ── Filters ──────────────────────────────────────────

document.getElementById('filters').addEventListener('click', (e) => {
  const btn = e.target.closest('.filter-btn');
  if (!btn) return;
  document.querySelectorAll('.filter-btn').forEach(b => b.classList.remove('active'));
  btn.classList.add('active');
  activeFilter = btn.dataset.filter;
  loadClips();
});

// ── Search ───────────────────────────────────────────

let debounceTimer;
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
  const deleteBtn = e.target.closest('.clip-delete');
  if (deleteBtn) {
    e.stopPropagation();
    const clipEl = deleteBtn.closest('.clip');
    if (clipEl) confirmDelete(clipEl.dataset.id);
    return;
  }

  const favBtn = e.target.closest('.fav');
  if (favBtn) {
    e.stopPropagation();
    const clipEl = favBtn.closest('.clip');
    if (clipEl) toggleFav(clipEl.dataset.id, favBtn);
    return;
  }

  const clipEl = e.target.closest('.clip');
  if (clipEl) copyClip(clipEl);
});

// ── Load & render ────────────────────────────────────

async function loadClips() {
  const search = searchEl.value.trim();
  let qs = '?limit=50';
  if (search) qs += `&search=${encodeURIComponent(search)}`;
  if (activeFilter !== 'all') qs += `&content_type=${activeFilter}`;

  try {
    const data = await apiGet(`/clips${qs}`);

    const clips = data.clips || [];
    const newIds = clips.map(c => c.id).join(',');

    if (newIds === lastClipIds && clipsEl.children.length > 0) return;
    lastClipIds = newIds;

    updateCount(clips.length);
    render(clips);
  } catch (e) {
    console.error('Failed to load clips:', e);
    showToast('Failed to load clips: ' + e.message);
  }
}

function updateCount(n) {
  clipCountEl.textContent = n > 0 ? `${n} clip${n !== 1 ? 's' : ''}` : '';
}

function render(clips) {
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
      textDiv.textContent = (clip.text_content || '').slice(0, 500);
      el.appendChild(textDiv);
    } else if (clip.content_type === 'image') {
      const imgDiv = document.createElement('div');
      imgDiv.className = 'clip-image';
      const img = document.createElement('img');
      img.loading = 'lazy';
      img.alt = 'Clip image';
      if (IS_TAURI) {
        // Load image through Rust proxy
        loadImageProxy(img, clip.id);
      } else {
        img.src = `${API}/clips/${encodeURIComponent(clip.id)}/content`;
      }
      imgDiv.appendChild(img);
      el.appendChild(imgDiv);
    }

    clipsEl.appendChild(el);
  });
}

async function loadImageProxy(img, clipId) {
  try {
    const b64 = await invoke('api_fetch_bytes', { path: `/clips/${clipId}/content` });
    img.src = `data:image/png;base64,${b64}`;
  } catch (e) {
    console.error('Failed to load image:', e);
  }
}

// ── Copy ─────────────────────────────────────────────

async function copyClip(el) {
  const id = el.dataset.id;
  const type = el.dataset.type;
  try {
    if (type === 'text') {
      const text = await apiGetText(`/clips/${encodeURIComponent(id)}/content`);
      if (IS_TAURI) {
        await invoke('copy_to_clipboard', { text });
      } else {
        await navigator.clipboard.writeText(text);
      }
    } else if (type === 'image') {
      if (IS_TAURI) {
        const b64 = await invoke('api_fetch_bytes', { path: `/clips/${id}/content` });
        const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
        await invoke('copy_image_to_clipboard', { pngData: Array.from(bytes) });
      } else {
        const resp = await fetch(`${API}/clips/${encodeURIComponent(id)}/content`);
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

async function toggleFav(id, el) {
  try {
    const data = await apiPatch(`/clips/${encodeURIComponent(id)}/favorite`);
    el.innerHTML = data.is_favorite ? '&#9733;' : '&#9734;';
    el.classList.toggle('is-fav', data.is_favorite);
  } catch (e) {
    console.error('Favorite toggle failed:', e);
    showToast('Failed to update favorite');
  }
}

// ── Delete ───────────────────────────────────────────

function confirmDelete(id) {
  pendingDeleteId = id;
  deleteModal.hidden = false;
}

function closeModal() {
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
    await apiDelete(`/clips/${encodeURIComponent(id)}`);
    const el = clipsEl.querySelector(`.clip[data-id="${CSS.escape(id)}"]`);
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

// ── Toast ────────────────────────────────────────────

function showToast(message) {
  const toast = document.createElement('div');
  toast.className = 'toast';
  toast.textContent = message;
  toastArea.appendChild(toast);
  setTimeout(() => {
    toast.classList.add('toast-out');
    toast.addEventListener('animationend', () => toast.remove());
  }, 2000);
}

// ── Relative time ────────────────────────────────────

function relativeTime(dateStr) {
  const now = Date.now();
  const then = new Date(dateStr).getTime();
  const diff = Math.max(0, now - then);
  const sec = Math.floor(diff / 1000);

  if (sec < 10) return 'just now';
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const days = Math.floor(hr / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(dateStr).toLocaleDateString();
}

// ── Settings (Tauri only) ────────────────────────────

const settingsBtn = document.getElementById('settings-btn');
const settingsModal = document.getElementById('settings-modal');
const settingsCancel = document.getElementById('settings-cancel');
const settingsSave = document.getElementById('settings-save');
const settingServerUrl = document.getElementById('setting-server-url');
const settingApiKey = document.getElementById('setting-api-key');
const settingInsecure = document.getElementById('setting-insecure');

if (IS_TAURI) {
  settingsBtn.hidden = false;

  settingsBtn.addEventListener('click', async () => {
    try {
      const cfg = await invoke('get_settings');
      settingServerUrl.value = cfg.server_url || '';
      settingApiKey.value = cfg.api_key || '';
      settingInsecure.checked = cfg.insecure || false;
    } catch (e) {
      console.warn('Failed to load settings:', e);
    }
    settingsModal.hidden = false;
  });

  settingsCancel.addEventListener('click', () => { settingsModal.hidden = true; });

  settingsModal.addEventListener('click', (e) => {
    if (e.target === settingsModal) settingsModal.hidden = true;
  });

  settingsSave.addEventListener('click', async () => {
    const cfg = {
      server_url: settingServerUrl.value.trim(),
      api_key: settingApiKey.value,
      insecure: settingInsecure.checked,
    };
    try {
      await invoke('save_settings', { settings: cfg });
      settingsModal.hidden = true;
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
