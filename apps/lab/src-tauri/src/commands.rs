//! IPC surface. Every command here is registered in `lib.rs`'s
//! `collect_commands!` and exported to `src/lib/bindings.ts` on debug builds —
//! adding one without registering it is a silent no-op on the frontend.

use tauri::State;

use crate::settings::AppSettings;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub fn settings_get(state: State<'_, AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

/// Whole-snapshot save — the frontend merges its patch and sends everything.
#[tauri::command]
#[specta::specta]
#[allow(unused_variables)]
pub fn settings_save(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    *state.settings.lock().unwrap() = settings;
    state.persist().map_err(|e| e.to_string())?;
    Ok(())
}
