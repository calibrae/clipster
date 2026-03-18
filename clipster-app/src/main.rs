// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppSettings {
    #[serde(default)]
    server_url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    insecure: bool,
}

static SETTINGS: OnceLock<Mutex<AppSettings>> = OnceLock::new();

fn settings_path() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("app.toml")
}

fn load_settings() -> AppSettings {
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
                server_url: "http://localhost:8743".into(),
                ..Default::default()
            }
        }
    }
}

fn save_settings_to_disk(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = toml::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(())
}

fn current_settings() -> AppSettings {
    SETTINGS
        .get()
        .and_then(|m| m.lock().ok())
        .map(|s| s.clone())
        .unwrap_or_default()
}

fn build_http_client(settings: &AppSettings) -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(settings.insecure)
        .build()
        .expect("failed to build HTTP client")
}

// ── API proxy ────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ApiRequest {
    method: String,
    path: String,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiResponse {
    status: u16,
    body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
}

/// Proxy all API calls through Rust (handles TLS, auth, self-signed certs)
#[tauri::command]
async fn api_request(req: ApiRequest) -> Result<ApiResponse, String> {
    let settings = current_settings();
    let client = build_http_client(&settings);
    let base = settings.server_url.trim_end_matches('/');
    let url = format!("{}{}", base, req.path);

    let mut builder = match req.method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        "PUT" => client.put(&url),
        _ => return Err(format!("unsupported method: {}", req.method)),
    };

    if !settings.api_key.is_empty() {
        builder = builder.bearer_auth(&settings.api_key);
    }

    if let Some(body) = req.body {
        builder = builder.header("content-type", "application/json").body(body);
    }

    let resp = builder.send().await.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = resp.text().await.map_err(|e| e.to_string())?;

    Ok(ApiResponse {
        status,
        body,
        content_type,
    })
}

/// Fetch raw binary content (for images) — returns base64
#[tauri::command]
async fn api_fetch_bytes(path: String) -> Result<String, String> {
    use base64::Engine;

    let settings = current_settings();
    let client = build_http_client(&settings);
    let base = settings.server_url.trim_end_matches('/');
    let url = format!("{}{}", base, path);

    let mut builder = client.get(&url);
    if !settings.api_key.is_empty() {
        builder = builder.bearer_auth(&settings.api_key);
    }

    let resp = builder.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

// ── Settings commands ────────────────────────────────

#[tauri::command]
fn get_settings() -> AppSettings {
    current_settings()
}

#[tauri::command]
fn save_settings(settings: AppSettings) -> Result<(), String> {
    save_settings_to_disk(&settings)?;
    if let Some(m) = SETTINGS.get() {
        if let Ok(mut s) = m.lock() {
            *s = settings;
        }
    }
    Ok(())
}

// ── Clipboard commands ───────────────────────────────

#[tauri::command]
fn copy_to_clipboard(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(&text).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn copy_image_to_clipboard(png_data: Vec<u8>) -> Result<(), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(&png_data));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());

    let img = arboard::ImageData {
        width: info.width as usize,
        height: info.height as usize,
        bytes: buf.into(),
    };

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_image(img).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Main ─────────────────────────────────────────────

fn main() {
    let settings = load_settings();
    SETTINGS.set(Mutex::new(settings)).ok();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .build(),
        )
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Show Clipster", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Clipster")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => toggle_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_window(tray.app_handle());
                    }
                })
                .build(app)?;

            use tauri_plugin_global_shortcut::ShortcutState;
            app.global_shortcut().on_shortcut(
                "CmdOrCtrl+Shift+V",
                move |app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        toggle_window(app);
                    }
                },
            )?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api_request,
            api_fetch_bytes,
            get_settings,
            save_settings,
            copy_to_clipboard,
            copy_image_to_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Clipster");
}

fn toggle_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}
