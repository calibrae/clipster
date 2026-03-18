# Clipster

Cross-platform clipboard manager with cloud sync. Self-hosted, built in Rust.

Copy on any device, paste on any other. Text and images, synced in real-time through your own server.

## Architecture

```
clipster/
  clipster-common/     # Shared types, models, config
  clipster-server/     # REST API + embedded web UI + SQLite
  clipster-agent/      # Background daemon, clipboard watcher
  clipster-cli/        # CLI interface
  clipster-app/        # Tauri desktop app (system tray)
  web/                 # Web UI (embedded at compile time)
  deploy/              # systemd service, install script, example config
```

## Quick Start

### 1. Build

```bash
cargo build --release --workspace
```

### 2. Setup Server

```bash
# Generate config + API key, print client connection params
clipster-server setup

# Or with TLS (auto-generates self-signed cert)
clipster-server setup --tls
```

This creates `server.toml` in the platform config directory and outputs:

```
server_url = "https://10.10.0.2:8743"
api_key = "clp_..."
insecure = true
```

### 3. Install as Daemon

```bash
# macOS (launchd), Linux (systemd --user), Windows (schtasks)
clipster-server install

# Check status
clipster-server status

# Remove
clipster-server uninstall
```

### 4. Connect Clients

**Agent** (background clipboard watcher):
```bash
clipster-agent --server https://10.10.0.2:8743 -k
```

**CLI**:
```bash
clipster list
clipster search "something"
clipster copy <clip-id>
```

**Desktop App** (Tauri):
```bash
cargo run -p clipster-app
# Then: Settings > Server URL
```

**Agent as macOS daemon**:
```bash
clipster-agent install    # launchd
clipster-agent status
clipster-agent uninstall
```

## Features

- **Clipboard sync** — text and images, across macOS/Linux/Windows
- **Web UI** — embedded in the server binary, dark theme, search, filters
- **Desktop app** — system tray with Cmd+Shift+V hotkey, native clipboard write
- **Deduplication** — content-hash based, 5s window
- **TLS** — built-in self-signed cert generation, no reverse proxy needed
- **Auth** — API key with timing-safe comparison
- **Soft delete** — clips are never hard-deleted
- **Favorites** — star clips to find them later

## API

All endpoints under `/api/v1`, authenticated via `Authorization: Bearer <key>`.

| Method | Path | Description |
|--------|------|-------------|
| POST | /clips | Create clip (JSON text, multipart image) |
| GET | /clips | List/search (`?limit=&offset=&type=&search=&device=`) |
| GET | /clips/:id | Get clip metadata |
| GET | /clips/:id/content | Get raw content |
| DELETE | /clips/:id | Soft-delete |
| PATCH | /clips/:id/favorite | Toggle favorite |
| GET | /health | Health check (unauthenticated) |

## Deployment

### Docker

```bash
docker compose up -d
```

### Systemd (Linux)

```bash
sudo deploy/install.sh
```

### Manual

```bash
clipster-server --bind 0.0.0.0:8743 --tls
```

## Config Files

Platform config directory (`~/.config/clipster/` on Linux, `~/Library/Application Support/com.clipster.clipster/` on macOS):

- `server.toml` — server config (bind, db_path, api_key, tls)
- `client.toml` — agent/CLI config (server_url, api_key, device_name)
- `app.toml` — desktop app config (server_url, api_key, insecure)

## Stack

Rust workspace: axum, SQLite (rusqlite), arboard, Tauri v2, reqwest, tokio, clap, rust-embed.

## License

MIT
