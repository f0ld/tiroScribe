//! User preferences, stored as JSON in the config directory. The frontend owns the shape;
//! the backend only persists it.

use std::path::PathBuf;

fn path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tiroScribe")
        .join("settings.json")
}

#[tauri::command]
pub fn get_settings() -> serde_json::Value {
    std::fs::read(path())
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
pub fn set_settings(settings: serde_json::Value) -> Result<(), String> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(p, data).map_err(|e| e.to_string())
}
