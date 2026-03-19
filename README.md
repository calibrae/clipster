<p align="center">
  <img src="assets/flux-clipster-steampunk.png" width="256" alt="Clipster — steampunk clipboard machine">
</p>
<h1 align="center">Clipster</h1>
<p align="center"><em>Your clipboard's cooler, bearded cousin who syncs across all your devices.</em></p>

<p align="center">
  <a href="https://github.com/calibrae/clipster/actions"><img src="https://github.com/calibrae/clipster/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/calibrae/clipster/releases/latest"><img src="https://img.shields.io/github/v/release/calibrae/clipster?color=a78bfa" alt="Release"></a>
  <img src="https://img.shields.io/badge/rust-stable-orange" alt="Rust">
  <img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue" alt="Platforms">
  <img src="https://img.shields.io/badge/macOS-signed%20%2B%20notarized-brightgreen" alt="Notarized">
  <img src="https://img.shields.io/badge/vibe-steampunk-cd7f32" alt="Steampunk">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
</p>

---

Self-hosted clipboard manager for people who copy things on one machine and desperately need them on another. Text, images, synced in real-time. No cloud accounts, no subscriptions, no telemetry. Just your server, your data, your clipboard.

```
+----------------+                +--------------------+                +----------------+
|    MacBook     |                |  Clipster Server   |                |   Windows PC   |
|                |----push------->|                    |<-----push------|                |
|  Cmd+C "hi"    |                |   SQLite + Web UI  |                |   Ctrl+C img   |
|                |<---poll--------|      :8743         |------poll----->|                |
|   sees img     |                |                    |                |   sees "hi"    |
+----------------+                +--------------------+                +----------------+
                                           |
                                        browser
                                     +---------+
                                     |  Web UI |
                                     +---------+
```

## Why Clipster?

Because you're tired of emailing yourself links. Because Slack DMs to yourself feel wrong. Because AirDrop only works when it feels like it. Because you have a homelab and you're going to use it, dammit.

## What's in the box

- **Clipboard sync** — text and images, real-time, macOS / Linux / Windows
- **Native desktop apps** — system tray on all platforms, `Cmd+Shift+V` / `Ctrl+Shift+V` hotkey, embedded sync agent
- **Web UI** — dark theme, search, filters, favorites, click-to-copy, zero-config auth (just works)
- **Self-hosted** — runs on a Raspberry Pi, a NAS, your homelab, or literally anything
- **Built-in TLS** — auto-generates self-signed certs. No nginx required. We're not animals
- **API key auth** — timing-safe validation, because we read the OWASP top 10
- **macOS signed + notarized** — no Gatekeeper warnings, no `xattr -cr`, just double-click
- **Windows MSI + NSIS installer** — proper install/uninstall, no loose EXEs
- **Linux AppImage + .deb** — works on any distro
- **Daemon support** — `install` / `uninstall` / `status` on every platform (launchd / systemd / schtasks)
- **Tiny binaries** — 3.3 MB server, 2 MB DMG. Smaller than your average node_modules
- **One command setup** — `clipster-server setup` generates everything, prints client config
- **TypeScript web UI** — strict types, 64 unit + integration tests, esbuild bundled
- **CI/CD pipeline** — builds all platforms, signs macOS, notarizes with Apple, publishes releases

## Installation

### Pre-built binaries

Grab the latest from [Releases](https://github.com/calibrae/clipster/releases/latest):

| Platform | Desktop App | Server + CLI |
|---|---|---|
| macOS (Apple Silicon) | `Clipster_macOS_aarch64.dmg` | included in DMG |
| macOS (Intel) | `Clipster_macOS_x86_64.dmg` | included in DMG |
| Linux (x86_64) | `Clipster_linux_x86_64.AppImage` / `.deb` | `clipster-linux-x64.tar.gz` |
| Windows (x86_64) | `Clipster_windows_x86_64.msi` / `_setup.exe` | `clipster-windows-x64.tar.gz` |

macOS DMGs are **signed and notarized** with a Developer ID certificate — no security warnings.

### Build from source

```bash
git clone https://github.com/calibrae/clipster.git
cd clipster
cd web && npm ci && npm run build && cd ..   # build web UI
cargo build --release --workspace            # build everything
cd clipster-app && cargo tauri build         # build desktop app
```

## Quick Start

### Step 1: Set up the server (30 seconds)

```bash
clipster-server setup --tls
```

It generates your config, creates an API key, and tells you exactly what to do next:

```
=== Clipster Server Setup Complete ===

Config:  /etc/clipster/server.toml
Bind:    0.0.0.0:8743
TLS:     true

--- Client Configuration ---

  server_url = "https://10.10.0.2:8743"
  api_key = "clp_2iogpBAyxAuPLjjTCkf..."
  insecure = true
```

Running as root? It creates a `clipster` system user, writes to `/etc/clipster/`, stores data in `/var/lib/clipster/`, and sets proper permissions. Running as a normal user? It uses your config directory instead.

### Step 2: Run it (or daemonize it)

```bash
clipster-server                # just run it
clipster-server install        # install as daemon (launchd / systemd / schtasks)
clipster-server status         # is it vibing?
clipster-server uninstall      # break up with it
```

### Step 3: Connect your devices

**Desktop app** — open from the DMG (macOS), MSI (Windows), or AppImage (Linux). Click the tray icon, open Settings, paste your server URL + API key. Done. Clipboard syncs automatically in the background.

The app lives in the system tray — no dock icon, no Cmd+Tab entry, no taskbar button. Click the tray icon to toggle the clip panel. Click outside to dismiss. `Cmd+Shift+V` / `Ctrl+Shift+V` to toggle from anywhere.

**Web UI** — open `http://your-server:8743` in a browser. No login needed — the embedded web UI is automatically trusted. External API clients need the Bearer token.

**Headless machines** (Linux / Windows servers without a GUI):
```bash
clipster-agent --server https://10.10.0.2:8743 -k
```

**CLI** (for the terminal purists):
```bash
clipster-cli list              # what's been copied lately?
clipster-cli search "password" # oh no
clipster-cli copy <clip-id>    # yoink it to your clipboard
```

## Architecture

```
clipster/
  clipster-common/     Shared types, models, config
  clipster-server/     REST API + embedded web UI + SQLite
  clipster-agent/      Headless sync daemon (for machines without a GUI)
  clipster-cli/        CLI tool
  clipster-app/        Tauri v2 desktop app (tray + embedded sync agent)
  web/                 TypeScript source + esbuild bundle
  deploy/              systemd service, Dockerfile, install script
```

**Server**: single binary. API, web UI, SQLite, TLS — all baked in. No nginx, no Postgres, no Docker required (Dockerfile included for those who can't help themselves).

**Client app**: also a single binary. System tray, clipboard watcher, sync agent, settings panel — one `.app` / `.exe` / AppImage to rule them all. The Tauri app proxies all API calls through Rust, so self-signed TLS works transparently.

## API

All endpoints under `/api/v1`. External clients auth via `Authorization: Bearer <key>`. The embedded web UI skips auth automatically.

| Method | Path | What it does |
|--------|------|-------------|
| `POST` | `/clips` | Create a clip (JSON for text, multipart for images) |
| `GET` | `/clips` | List / search (`?limit=&offset=&type=&search=&device=`) |
| `GET` | `/clips/:id` | Get clip metadata |
| `GET` | `/clips/:id/content` | Get the actual content |
| `DELETE` | `/clips/:id` | Soft-delete (we don't do hard deletes, we're not monsters) |
| `PATCH` | `/clips/:id/favorite` | Star it for later |
| `GET` | `/health` | Is the server alive? (no auth needed) |

## Deployment

### Docker

```bash
docker compose up -d
```

### systemd (Linux, as root)

```bash
sudo clipster-server setup
sudo clipster-server install
```

### systemd (Linux, as user)

```bash
clipster-server setup
clipster-server install
```

### launchd (macOS)

```bash
clipster-server install
```

### Windows

```bash
clipster-server install   # creates a scheduled task
```

## Security

- **API key auth** with constant-time comparison (`subtle` crate)
- **Built-in TLS** with auto-generated self-signed certificates
- **Request body limits** (50 MB max)
- **Security headers** (X-Content-Type-Options, X-Frame-Options, Referrer-Policy)
- **Hardened systemd unit** (ProtectSystem=strict, NoNewPrivileges, PrivateTmp)
- **macOS hardened runtime** + notarization
- **Web UI auto-auth** — same-origin requests skip API key (no credentials in the browser)
- **Error sanitization** — internal errors logged server-side, generic messages to clients

## Testing

```bash
cargo test --workspace --exclude clipster-app   # 29 Rust tests
cd web && npm test                               # 35 TypeScript tests
```

64 tests total: models, hashing, serde, database CRUD, API endpoints, time formatting, API path construction, browser client integration, Tauri proxy integration.

## Config files

| File | Who uses it | What's in it |
|------|------------|-------------|
| `server.toml` | Server | `bind`, `db_path`, `api_key`, `tls` |
| `app.toml` | Desktop app | `server_url`, `api_key`, `insecure`, `sync_enabled` |
| `client.toml` | Agent / CLI | `server_url`, `api_key`, `device_name` |

Lives in `~/.config/clipster/` (Linux), `~/Library/Application Support/com.clipster.clipster/` (macOS), or `%APPDATA%\clipster\` (Windows). Root installs use `/etc/clipster/`.

## Built with

Rust, TypeScript, axum, SQLite, Tauri v2, arboard, reqwest, tokio, rustls, esbuild, vitest, clap. Zero JavaScript frameworks were harmed in the making of this project.

## License

MIT — copy it, fork it, paste it. That's kind of our whole thing.
