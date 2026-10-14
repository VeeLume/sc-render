//! Managed app state. One place that knows where data lives on disk, so
//! nothing else has to resolve paths.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::settings::AppSettings;
use crate::store;

pub struct AppState {
    /// Tauri's per-app data dir, created at startup.
    pub data_dir: PathBuf,
    pub settings: Mutex<AppSettings>,
}

impl AppState {
    pub fn load(data_dir: &Path) -> Self {
        let settings = store::read_json(&data_dir.join("settings.json"));
        Self {
            data_dir: data_dir.to_path_buf(),
            settings: Mutex::new(settings),
        }
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    /// Write the current snapshot to disk.
    pub fn persist(&self) -> std::io::Result<()> {
        let snapshot = self.settings.lock().unwrap().clone();
        store::write_json(&self.settings_path(), &snapshot)
    }
}
