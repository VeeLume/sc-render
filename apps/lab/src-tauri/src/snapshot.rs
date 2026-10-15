//! Dev snapshots: rendered cells and their logs, saved under
//! `<app data>/snapshots/` for comparing renders across changes.

use tauri::Manager;

/// Save a rendered cell as `<app data>/snapshots/<name>.png`; returns the path.
#[tauri::command]
#[specta::specta]
pub fn save_snapshot(app: tauri::AppHandle, name: String, png: Vec<u8>) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("snapshots");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{safe}.png"));
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Save a render log next to the snapshots as `<name>.txt`.
#[tauri::command]
#[specta::specta]
pub fn save_log(app: tauri::AppHandle, name: String, text: String) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("snapshots");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    std::fs::write(dir.join(format!("{safe}.txt")), text).map_err(|e| e.to_string())
}
