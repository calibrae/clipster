//! Tauri commands beyond the core API: clipboard helpers + peer trust.

use crate::core;
use clipster_core::db::peers::{PeerRecord, TrustStatus};
use serde::Serialize;

#[tauri::command]
pub fn copy_to_clipboard(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(&text).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn copy_image_to_clipboard(png_data: Vec<u8>) -> Result<(), String> {
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

#[derive(Debug, Serialize)]
pub struct IdentityInfo {
    pub device_id: String,
    pub device_name: String,
}

#[tauri::command]
pub fn get_identity() -> IdentityInfo {
    let c = core();
    IdentityInfo {
        device_id: c.identity.device_id.clone(),
        device_name: c.identity.device_name.clone(),
    }
}

#[tauri::command]
pub fn list_peers() -> Result<Vec<PeerRecord>, String> {
    core().db.list_peers().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_pending_peers() -> Result<Vec<PeerRecord>, String> {
    let all = core().db.list_peers().map_err(|e| e.to_string())?;
    Ok(all
        .into_iter()
        .filter(|p| p.trust_status == TrustStatus::Pending)
        .collect())
}

#[tauri::command]
pub fn trust_peer(device_id: String) -> Result<(), String> {
    core().trust.trust(&device_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn revoke_peer(device_id: String) -> Result<(), String> {
    core().trust.reject(&device_id).map_err(|e| e.to_string())
}
