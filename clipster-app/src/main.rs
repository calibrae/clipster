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

fn main() {
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

fn show_window_at(app: &tauri::AppHandle, tray_x: f64, tray_y: f64) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            return;
        }

        let w = 400.0_f64;
        #[allow(unused)]
        let h = 600.0_f64;

        if let Ok(Some(monitor)) = window.primary_monitor() {
            let screen = monitor.size();
            let scale = monitor.scale_factor();
            let screen_w = screen.width as f64 / scale;

            #[cfg(target_os = "macos")]
            let (x, y) = {
                let x = (tray_x - w / 2.0).clamp(8.0, screen_w - w - 8.0);
                (x, tray_y + 8.0)
            };

            #[cfg(not(target_os = "macos"))]
            let (x, y) = {
                let x = (tray_x - w / 2.0).clamp(8.0, screen_w - w - 8.0);
                (x, (tray_y - h - 8.0).max(8.0))
            };

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
                let screen = monitor.size();
                let scale = monitor.scale_factor();
                let screen_w = screen.width as f64 / scale;

                #[cfg(target_os = "macos")]
                let (x, y) = (screen_w - 400.0 - 12.0, 30.0);

                #[cfg(not(target_os = "macos"))]
                let (x, y) = {
                    let screen_h = screen.height as f64 / scale;
                    (screen_w - 400.0 - 12.0, screen_h - 600.0 - 48.0)
                };

                let _ = window.set_position(tauri::Position::Logical(
                    tauri::LogicalPosition::new(x, y),
                ));
            }
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}
