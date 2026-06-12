// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod commands;
mod peer_listener;
mod settings;
mod sync;

use clipster_core::ClipsterCore;
use clipster_core::db::Database;
use clipster_core::discovery::{Announcer, Browser};
use clipster_core::identity::Identity;
use clipster_core::sync::SyncEngine;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub static CORE: OnceLock<Arc<ClipsterCore>> = OnceLock::new();

pub fn core() -> Arc<ClipsterCore> {
    CORE.get().expect("core not initialized").clone()
}

fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn init_core() -> anyhow::Result<Arc<ClipsterCore>> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let db_path = dir.join("clipster.db");
    let image_dir = dir.join("images");
    std::fs::create_dir_all(&image_dir)?;

    let db = Database::open(db_path.to_str().unwrap())?;
    db.migrate()?;
    let identity = Identity::load_or_create(&dir, None)?;
    let core = Arc::new(ClipsterCore::new(db, identity, image_dir));
    Ok(core)
}

fn install_panic_hook() {
    let log_path = log_path();
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic>".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".into());
        let entry = format!(
            "[{}] PANIC at {location}\n  message: {payload}\n  backtrace:\n{backtrace}\n\n",
            chrono::Utc::now().to_rfc3339()
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = f.write_all(entry.as_bytes());
        }
        eprintln!("{entry}");
    }));
}

fn log_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().join("Library/Logs/Clipster/panic.log"))
            .unwrap_or_else(|| PathBuf::from("clipster-panic.log"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        directories::ProjectDirs::from("com", "clipster", "clipster")
            .map(|d| d.data_local_dir().join("panic.log"))
            .unwrap_or_else(|| PathBuf::from("clipster-panic.log"))
    }
}

fn main() {
    install_panic_hook();

    // Hide from Cmd+Tab on macOS
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        let mtm = unsafe { objc2::MainThreadMarker::new_unchecked() };
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }

    let core = init_core().expect("failed to init clipster core");
    CORE.set(core.clone()).ok();
    settings::load_or_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            let show = MenuItem::with_id(app, "show", "Show Clipster", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            #[cfg(target_os = "macos")]
            let tray_icon = {
                let bytes = include_bytes!("../icons/tray-icon.png");
                tauri::image::Image::from_bytes(bytes).expect("failed to load tray icon")
            };
            #[cfg(not(target_os = "macos"))]
            let tray_icon = app.default_window_icon().unwrap().clone();

            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(cfg!(target_os = "macos"))
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
                        position,
                        ..
                    } = event
                    {
                        show_window_at(tray.app_handle(), position.x, position.y);
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

            // ── Spawn the background runtime (clipboard watcher + peer listener + sync) ─────
            let core = core.clone();
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
                rt.block_on(async move {
                    spawn_background(core, app_handle).await;
                    // Park forever (the tasks live on the runtime)
                    futures_park().await;
                });
            });

            let window = app.get_webview_window("main").unwrap();
            let w = window.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = w.hide();
                }
                tauri::WindowEvent::Focused(false) => {
                    let _ = w.hide();
                }
                _ => {}
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Core data API (replaces old http-proxy api_request)
            api::api_request,
            api::api_fetch_bytes,
            // Settings (legacy)
            settings::get_settings,
            settings::save_settings,
            // Clipboard helpers
            commands::copy_to_clipboard,
            commands::copy_image_to_clipboard,
            // Peer management (TOFU)
            commands::list_peers,
            commands::list_pending_peers,
            commands::trust_peer,
            commands::revoke_peer,
            commands::get_identity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Clipster");
}

async fn spawn_background(core: Arc<ClipsterCore>, app_handle: tauri::AppHandle) {
    // mDNS announce + browse
    let peer_port = settings::ensure_peer_port();
    let announcer = match Announcer::start(
        &core.identity.device_name,
        peer_port,
        &core.identity.device_id,
        &core.identity.device_name,
        env!("CARGO_PKG_VERSION"),
        &["app", "images"],
    ) {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!(error = %e, "mDNS announce failed");
            None
        }
    };
    let browser = match Browser::start(core.identity.device_id.clone()) {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::warn!(error = %e, "mDNS browse failed");
            None
        }
    };
    std::mem::forget(announcer); // keep alive for process lifetime

    let sync_engine = SyncEngine::spawn(core.clone());
    if let Some(b) = &browser {
        sync_engine.attach_discovery(b.subscribe());
    }
    std::mem::forget(browser);
    drop(sync_engine);

    // Peer-facing TLS listener
    let core2 = core.clone();
    tokio::spawn(async move {
        if let Err(e) = peer_listener::run(core2, peer_port).await {
            tracing::error!(error = %e, "peer listener exited");
        }
    });

    // Forward pending-peer events to JS
    let app2 = app_handle.clone();
    let mut pending_rx = core.trust.pending_events();
    tokio::spawn(async move {
        while let Ok(event) = pending_rx.recv().await {
            let _ = app2.emit(
                "peer-pending",
                serde_json::json!({
                    "device_id": event.device_id,
                    "name": event.name,
                    "addr": event.addr,
                }),
            );
        }
    });

    // Clipboard watcher
    let core3 = core.clone();
    tokio::spawn(async move {
        sync::run_watch_loop(core3).await;
    });
}

async fn futures_park() {
    use std::future::pending;
    let _: () = pending().await;
}

/// Compute clamped logical (x, y) for the window panel. Always safe for
/// degenerate / virtual / screen-shared displays — falls back to (8, 8).
fn safe_position(
    monitor_size_w: f64,
    monitor_size_h: f64,
    scale: f64,
    win_w: f64,
    win_h: f64,
    tray_x: f64,
    tray_y: f64,
    macos_layout: bool,
) -> (f64, f64) {
    let scale = if scale.is_finite() && scale > 0.1 { scale } else { 1.0 };
    let screen_w = (monitor_size_w / scale).max(win_w + 16.0);
    let screen_h = (monitor_size_h / scale).max(win_h + 16.0);

    let max_x = (screen_w - win_w - 8.0).max(8.0);
    let raw_x = if tray_x.is_finite() { tray_x - win_w / 2.0 } else { 8.0 };
    let x = raw_x.clamp(8.0, max_x);

    let y = if macos_layout {
        if tray_y.is_finite() { tray_y + 8.0 } else { 8.0 }
    } else {
        let raw_y = if tray_y.is_finite() { tray_y - win_h - 8.0 } else { 8.0 };
        raw_y.max(8.0).min((screen_h - win_h - 8.0).max(8.0))
    };
    (x, y)
}

fn show_window_at(app: &tauri::AppHandle, tray_x: f64, tray_y: f64) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            return;
        }

        let w = 400.0_f64;
        let h = 600.0_f64;

        if let Ok(Some(monitor)) = window.primary_monitor() {
            let size = monitor.size();
            let (x, y) = safe_position(
                size.width as f64,
                size.height as f64,
                monitor.scale_factor(),
                w,
                h,
                tray_x,
                tray_y,
                cfg!(target_os = "macos"),
            );

            let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                x, y,
            )));
        }

        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            if let Ok(Some(monitor)) = window.primary_monitor() {
                let size = monitor.size();
                let scale = monitor.scale_factor();
                let scale = if scale.is_finite() && scale > 0.1 { scale } else { 1.0 };
                let screen_w = (size.width as f64 / scale).max(416.0);
                let screen_h = (size.height as f64 / scale).max(616.0);

                #[cfg(target_os = "macos")]
                let (x, y) = ((screen_w - 400.0 - 12.0).max(8.0), 30.0);

                #[cfg(not(target_os = "macos"))]
                let (x, y) = (
                    (screen_w - 400.0 - 12.0).max(8.0),
                    (screen_h - 600.0 - 48.0).max(8.0),
                );
                #[cfg(target_os = "macos")]
                let _ = screen_h;

                let _ = window.set_position(tauri::Position::Logical(
                    tauri::LogicalPosition::new(x, y),
                ));
            }
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::safe_position;

    #[test]
    fn degenerate_screen_does_not_panic() {
        // The screen-sharing virtual display case: width effectively reports as 1
        let (x, y) = safe_position(1.0, 1.0, 1.0, 400.0, 600.0, 100.0, 30.0, true);
        assert_eq!(x, 8.0);
        assert_eq!(y, 38.0);
    }

    #[test]
    fn nan_inputs_do_not_panic() {
        let (x, y) = safe_position(f64::NAN, f64::NAN, f64::NAN, 400.0, 600.0, f64::NAN, f64::NAN, true);
        assert!(x.is_finite());
        assert!(y.is_finite());
    }

    #[test]
    fn normal_screen_centers_under_tray() {
        // 1440 logical wide, tray at x=1300
        let (x, _) = safe_position(2880.0, 1800.0, 2.0, 400.0, 600.0, 1300.0, 24.0, true);
        // Should be roughly 1300 - 200 = 1100, clamped to [8, 1032]
        assert_eq!(x, 1032.0);
    }
}
