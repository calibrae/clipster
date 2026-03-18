<p align="center">
  <img src="assets/clipster.png" width="256" alt="Clipster — a clipboard hipster">
</p>
<h1 align="center">Clipster</h1>
<p align="center"><em>Your clipboard's cooler, bearded cousin who syncs across all your devices.</em></p>

<p align="center">
  <a href="https://github.com/calibrae/clipster/actions"><img src="https://github.com/calibrae/clipster/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/calibrae/clipster/releases/latest"><img src="https://img.shields.io/github/v/release/calibrae/clipster?color=a78bfa" alt="Release"></a>
  <img src="https://img.shields.io/badge/rust-stable-orange" alt="Rust">
  <img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue" alt="Platforms">
  <img src="https://img.shields.io/badge/vibe-artisanal-ff69b4" alt="Artisanal">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
</p>

---

Self-hosted clipboard manager for people who copy things on one machine and desperately need them on another. Text, images, synced in real-time. No cloud accounts, no subscriptions, no telemetry. Just your server, your data, your clipboard — with a man bun.

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
- **Desktop app** — system tray, `Cmd+Shift+V` hotkey, syncs in the background
- **Web UI** — dark theme, search, filters, favorites, click-to-copy
- **Self-hosted** — runs on a Raspberry Pi, a NAS, your homelab, or literally anything
- **Built-in TLS** — auto-generates self-signed certs. No nginx required. We're not animals
- **API key auth** — timing-safe validation, because we read the OWASP top 10
- **Daemon support** — `install` / `uninstall` / `status` on every platform
- **Tiny binaries** — 3.3 MB server, 2 MB DMG. Smaller than your average node_modules
- **One command setup** — `clipster-server setup` does everything

## Installation

### Pre-built binaries

Grab the latest from [Releases](https://github.com/calibrae/clipster/releases/latest):

| Platform | Desktop App | Server + CLI |
|---|---|---|
| macOS (Apple Silicon) | `Clipster_macOS_aarch64.dmg` | included in DMG |
| macOS (Intel) | `Clipster_macOS_x86_64.dmg` | included in DMG |
| Linux (x86_64) | `Clipster_linux_x86_64.AppImage` / `.deb` | `clipster-linux-x64.tar.gz` |
| Windows (x86_64) | `Clipster_windows_x86_64.msi` / `_setup.exe` | `clipster-windows-x64.tar.gz` |

### Build from source

```bash
git clone https://github.com/calibrae/clipster.git
cd clipster
cargo build --release --workspace
```

## Quick Start

### Step 1: Set up the server (30 seconds)

```bash
clipster-server setup --tls
```

It generates your config, creates an API key, and tells you exactly what to do next:

```
=== Clipster Server Setup Complete ===

Config:  ~/.config/clipster/server.toml
Bind:    0.0.0.0:8743
TLS:     true

--- Client Configuration ---

  server_url = "https://10.10.0.2:8743"
  api_key = "clp_2iogpBAyxAuPLjjTCkf..."
  insecure = true

--- Quick Start ---

  clipster-agent --server https://10.10.0.2:8743 -k
```

### Step 2: Run it (or daemonize it)

```bash
clipster-server                # just run it
clipster-server install        # or install as a daemon (launchd / systemd / schtasks)
clipster-server status         # is it vibing?
clipster-server uninstall      # break up with it
```

### Step 3: Connect your devices

**Desktop app** (macOS) — open `Clipster.app` from the DMG, click the tray icon, hit the gear, paste your server URL + API key. That's it. Your clipboard now has a social life.

**Headless machines** (Linux / Windows servers):
```bash
clipster-agent --server https://10.10.0.2:8743 -k
```

**CLI** (for the terminal purists):
```bash
clipster-cli list              # what's been copied lately?
clipster-cli search "password" # oh no
clipster-cli copy <clip-id>    # yoink it to your clipboard
```

**Web UI** — open `https://your-server:8743` in a browser. Yes, it's dark mode. We're not savages.

## Architecture

```
clipster/
  clipster-common/     Shared types, models, config
  clipster-server/     REST API + embedded web UI + SQLite
  clipster-agent/      Headless sync daemon (for machines without a GUI)
  clipster-cli/        CLI tool
  clipster-app/        Tauri v2 desktop app (tray + embedded sync agent)
  web/                 HTML/CSS/JS (compiled into the binaries)
  deploy/              systemd service, Dockerfile, install script
```

**Server**: single binary. API, web UI, SQLite, TLS — all baked in. No nginx, no Postgres, no Docker required (Dockerfile included for those who can't help themselves).

**Client app**: also a single binary. Tray icon, clipboard watcher, sync, settings — one `.app` or `.exe` to rule them all.

## API

All endpoints under `/api/v1`. Auth via `Authorization: Bearer <key>`.

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

### Docker (for the containerized lifestyle)

```bash
docker compose up -d
```

### systemd (for the Linux faithful)

```bash
sudo deploy/install.sh
```

### Manual (for the free spirits)

```bash
clipster-server --bind 0.0.0.0:8743 --tls
```

## Config files

| File | Who uses it | What's in it |
|------|------------|-------------|
| `server.toml` | Server | `bind`, `db_path`, `api_key`, `tls` |
| `app.toml` | Desktop app | `server_url`, `api_key`, `insecure`, `sync_enabled` |
| `client.toml` | Agent / CLI | `server_url`, `api_key`, `device_name` |

Lives in `~/.config/clipster/` (Linux) or `~/Library/Application Support/com.clipster.clipster/` (macOS).

## Built with

Rust, axum, SQLite, Tauri v2, arboard, reqwest, tokio, rustls, clap. Zero JavaScript frameworks were harmed in the making of this project.

## License

MIT — copy it, fork it, paste it. That's kind of our whole thing.
