//! App-local settings persisted in `app.toml`.
//!
//! Legacy fields (`server_url`, `api_key`, `insecure`) are kept for one-shot
//! bootstrap when migrating users who previously talked to a central server
//! (see Phase 4 — auto-pair-with-legacy-server). New deployments only need
//! `local_peer_port` and `sync_enabled`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub insecure: bool,
    #[serde(default = "default_true")]
    pub sync_enabled: bool,
    /// Persisted peer-API port so other peers see the same address across restarts.
    #[serde(default)]
    pub local_peer_port: u16,
}

fn default_true() -> bool { true }

static SETTINGS: OnceLock<Mutex<AppSettings>> = OnceLock::new();

pub fn settings_path() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("app.toml")
}

pub fn load_or_init() {
    let s = load_from_disk();
    SETTINGS.set(Mutex::new(s)).ok();
}

fn load_from_disk() -> AppSettings {
    let path = settings_path();
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        let client_path = path.with_file_name("client.toml");
        if client_path.exists() {
            std::fs::read_to_string(&client_path)
                .ok()
                .and_then(|s| toml::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            AppSettings {
                sync_enabled: true,
                ..Default::default()
            }
        }
    }
}

pub fn current() -> AppSettings {
    SETTINGS
        .get()
        .and_then(|m| m.lock().ok())
        .map(|s| s.clone())
        .unwrap_or_default()
}

pub fn save_to_disk(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = toml::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    if let Some(m) = SETTINGS.get() {
        if let Ok(mut w) = m.lock() {
            *w = settings.clone();
        }
    }
    Ok(())
}

/// Returns the persisted peer port, allocating an ephemeral one on first run.
pub fn ensure_peer_port() -> u16 {
    let mut s = current();
    if s.local_peer_port != 0 {
        return s.local_peer_port;
    }
    // Pick an ephemeral port by binding briefly.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(38743);
    s.local_peer_port = port;
    let _ = save_to_disk(&s);
    port
}

#[tauri::command]
pub fn get_settings() -> AppSettings {
    current()
}

#[tauri::command]
pub fn save_settings(settings: AppSettings) -> Result<(), String> {
    save_to_disk(&settings)
}
