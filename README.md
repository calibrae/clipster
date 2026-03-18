<p align="center">
  <h1 align="center">📋 Clipster</h1>
  <p align="center">Copy here, paste there. Your clipboard, everywhere.</p>
</p>

<p align="center">
  <a href="https://github.com/calibrae/clipster/actions"><img src="https://github.com/calibrae/clipster/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/calibrae/clipster/releases/latest"><img src="https://img.shields.io/github/v/release/calibrae/clipster?color=a78bfa" alt="Release"></a>
  <img src="https://img.shields.io/badge/rust-stable-orange" alt="Rust">
  <img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue" alt="Platforms">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
</p>

---

Self-hosted clipboard manager with cloud sync. Copy text or images on any machine, access them from any other. Built in Rust, runs on your hardware.

```
┌──────────────┐         ┌──────────────────┐         ┌──────────────┐
│   MacBook    │         │  Clipster Server  │         │  Windows PC  │
│              │──push──▶│                   │◀──push──│              │
│  Cmd+C "hi"  │         │  SQLite + Web UI  │         │  Ctrl+C 🖼️  │
│              │◀──poll──│   :8743           │──poll──▶│              │
│  sees 🖼️     │         │                   │         │  sees "hi"   │
└──────────────┘         └──────────────────┘         └──────────────┘
                                  ▲
                                  │ browser
                              ┌───┴───┐
                              │ Web UI │
                              └───────┘
```

## Features

- **Clipboard sync** — text and images, real-time across macOS / Linux / Windows
- **Desktop app** — system tray, global hotkey (Cmd+Shift+V), embedded sync agent
- **Web UI** — dark theme, search, filters, favorites, click-to-copy
- **Self-hosted** — your server, your data, your rules
- **Built-in TLS** — auto-generated self-signed certs, no reverse proxy needed
- **API key auth** — timing-safe Bearer token validation
- **Daemon support** — `install` / `uninstall` / `status` on every platform
- **Small binaries** — server 3.3 MB, client 2 MB DMG (strip + LTO)
- **Single setup** — `clipster-server setup` generates everything, prints client config

## Installation

### Pre-built binaries

Download from [GitHub Releases](https://github.com/calibrae/clipster/releases/latest):

| Platform | File | Contents |
|---|---|---|
| macOS (Apple Silicon) | `Clipster_aarch64.dmg` | Desktop app + server + CLI |
| macOS (Intel) | `Clipster_x86_64.dmg` | Desktop app + server + CLI |
| Linux (x86_64) | `clipster-linux-x64.tar.gz` | Server + agent + CLI |
| Windows (x86_64) | `clipster-windows-x64.tar.gz` | Server + agent + CLI |

### Build from source

```bash
cargo build --release --workspace
```

## Quick Start

### 1. Setup the server

```bash
clipster-server setup --tls
```

Generates config + API key, prints everything you need:

```
=== Clipster Server Setup Complete ===

Config:  ~/.config/clipster/server.toml
Bind:    0.0.0.0:8743
TLS:     true

--- Client Configuration ---

  server_url = "https://10.10.0.2:8743"
  api_key = "clp_2iogpBAyxAuPLjjTCkf..."
  insecure = true
```

### 2. Run it

```bash
# Just run it
clipster-server

# Or install as a daemon
clipster-server install   # launchd (macOS) / systemd (Linux) / schtasks (Windows)
clipster-server status    # check it's running
```

### 3. Connect clients

**Desktop app** (macOS) — open `Clipster.app`, click the tray icon, go to Settings, paste your server URL + API key. Done. Clipboard syncs automatically.

**Headless** (Linux/Windows servers):
```bash
clipster-agent --server https://10.10.0.2:8743 -k
```

**CLI**:
```bash
clipster-cli list              # recent clips
clipster-cli search "foo"      # search
clipster-cli copy <clip-id>    # copy to local clipboard
```

**Web UI** — just open `https://your-server:8743` in a browser.

## Architecture

```
clipster/
  clipster-common/     Shared types, models, config
  clipster-server/     REST API + embedded web UI + SQLite
  clipster-agent/      Headless sync daemon (for servers without a GUI)
  clipster-cli/        CLI interface
  clipster-app/        Tauri v2 desktop app (tray + embedded sync)
  web/                 HTML/CSS/JS (compiled into server + app)
  deploy/              systemd service, Dockerfile, install script
```

**Server** is a single binary: API, web UI, SQLite, TLS — all built in. No nginx, no Postgres, no Docker required (but a Dockerfile is included if you want it).

**Client app** is also a single binary: system tray, clipboard watcher, sync agent, settings — all in one `.app` / `.exe`.

## API

All endpoints under `/api/v1`. Authenticated via `Authorization: Bearer <key>`.

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/clips` | Create clip (JSON for text, multipart for images) |
| `GET` | `/clips` | List / search (`?limit=&offset=&type=&search=&device=`) |
| `GET` | `/clips/:id` | Get clip metadata |
| `GET` | `/clips/:id/content` | Get raw content |
| `DELETE` | `/clips/:id` | Soft-delete |
| `PATCH` | `/clips/:id/favorite` | Toggle favorite |
| `GET` | `/health` | Health check (no auth required) |

## Deployment

### Docker

```bash
docker compose up -d
```

### systemd (Linux)

```bash
sudo deploy/install.sh
```

### Manual

```bash
clipster-server --bind 0.0.0.0:8743 --tls
```

## Config

Platform config directory (`~/.config/clipster/` on Linux, `~/Library/Application Support/com.clipster.clipster/` on macOS):

| File | Used by | Key settings |
|------|---------|-------------|
| `server.toml` | Server | `bind`, `db_path`, `api_key`, `tls` |
| `app.toml` | Desktop app | `server_url`, `api_key`, `insecure`, `sync_enabled` |
| `client.toml` | Agent / CLI | `server_url`, `api_key`, `device_name` |

## Stack

Rust, axum, SQLite, Tauri v2, arboard, reqwest, tokio, rustls, clap.

## License

MIT
