//! The item catalog: DataCore → render recipes, built once on a background
//! thread at startup (it takes ~40 s); commands wait for it.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use render_assets::{Catalog, ChildSpec, InventoryNode, ItemSummary, SceneItem};
use tauri::{AppHandle, Manager};

use crate::render::RenderState;

#[derive(Default)]
pub struct CatalogState {
    catalog: OnceLock<Result<Arc<Catalog>, String>>,
}

/// Start building the catalog; call once from `setup`.
pub fn start_loading(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let loaded = app
            .state::<RenderState>()
            .assets()
            .and_then(|a| Catalog::load(&a).map_err(|e| e.to_string()))
            .map(Arc::new);
        let _ = app.state::<CatalogState>().catalog.set(loaded);
    });
}

/// Block until the catalog is built.
fn wait(app: &AppHandle) -> Result<Arc<Catalog>, String> {
    let state = app.state::<CatalogState>();
    loop {
        if let Some(c) = state.catalog.get() {
            return c.clone();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// Items whose display or record name contains every word of `query`.
#[tauri::command]
#[specta::specta]
pub async fn search_items(app: AppHandle, query: String) -> Result<Vec<ItemSummary>, String> {
    blocking(move || Ok(wait(&app)?.search(&query, 200))).await
}

/// The render scene of an item; `children` replaces its default loadout.
#[tauri::command]
#[specta::specta]
pub async fn item_scene(
    app: AppHandle,
    guid: String,
    children: Option<Vec<ChildSpec>>,
) -> Result<SceneItem, String> {
    blocking(move || {
        let catalog = wait(&app)?;
        let assets = app.state::<RenderState>().assets()?;
        catalog
            .scene(&assets, &guid, children.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
}

/// A Starlume inventory dump, resolved; by default the dev build's
/// `%APPDATA%/starlume-dev/dossier/inventory.json`.
#[tauri::command]
#[specta::specta]
pub async fn load_inventory(
    app: AppHandle,
    path: Option<String>,
) -> Result<Vec<InventoryNode>, String> {
    blocking(move || {
        let path = match path {
            Some(p) => p.into(),
            None => app
                .path()
                .data_dir()
                .map_err(|e| e.to_string())?
                .join("starlume-dev/dossier/inventory.json"),
        };
        let json =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        wait(&app)?.inventory(&json).map_err(|e| e.to_string())
    })
    .await
}
