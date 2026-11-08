//! The item catalog: DataCore → render recipes, built once on a background
//! thread at startup (it takes ~40 s); commands wait for it.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use lab_catalog::{
    Body, Catalog, ChildSpec, ExposureDirection, HangarShip, InventoryNode, ItemSummary,
    ShipSummary, ShotResult, ShotSpec, WeaponSummary,
};
use sc_render::SceneItem;
use tauri::{AppHandle, Manager};
use tauri_plugin_sc_render::ScRenderExt;

#[derive(Default)]
pub struct CatalogState {
    catalog: OnceLock<Result<Arc<Catalog>, String>>,
}

/// Start building the catalog; call once from `setup`.
pub fn start_loading(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let loaded = app
            .sc_render()
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

/// Every item with a recipe, by label.
#[tauri::command]
#[specta::specta]
pub async fn list_items(app: AppHandle) -> Result<Vec<ItemSummary>, String> {
    blocking(move || Ok(wait(&app)?.all())).await
}

/// Items whose display or record name contains every word of `query`.
#[tauri::command]
#[specta::specta]
pub async fn search_items(app: AppHandle, query: String) -> Result<Vec<ItemSummary>, String> {
    blocking(move || Ok(wait(&app)?.search(&query, 200))).await
}

/// The render scene of an item; `children` replaces its default loadout;
/// `body` picks armor and clothing variants (default female).
#[tauri::command]
#[specta::specta]
pub async fn item_scene(
    app: AppHandle,
    guid: String,
    children: Option<Vec<ChildSpec>>,
    body: Option<Body>,
) -> Result<SceneItem, String> {
    blocking(move || {
        let catalog = wait(&app)?;
        let assets = app.sc_render().assets()?;
        catalog
            .scene(
                &assets,
                &guid,
                children.as_deref(),
                body.unwrap_or_default(),
            )
            .map_err(|e| e.to_string())
    })
    .await
}

/// The listed player ships and vehicles, by label.
#[tauri::command]
#[specta::specta]
pub async fn list_ships(app: AppHandle) -> Result<Vec<ShipSummary>, String> {
    blocking(move || Ok(wait(&app)?.ships())).await
}

/// A ship with its stock loadout, ready to draw, with its damage tooling.
#[tauri::command]
#[specta::specta]
pub async fn ship_view(app: AppHandle, guid: String) -> Result<HangarShip, String> {
    blocking(move || {
        let catalog = wait(&app)?;
        let assets = app.sc_render().assets()?;
        catalog.ship(&assets, &guid).map_err(|e| e.to_string())
    })
    .await
}

/// Every ship gun with damage.
#[tauri::command]
#[specta::specta]
pub async fn ship_weapons(app: AppHandle) -> Result<Vec<WeaponSummary>, String> {
    blocking(move || Ok(wait(&app)?.ship_weapons())).await
}

/// One shot at a ship's stock loadout.
#[tauri::command]
#[specta::specta]
pub async fn ship_hit(app: AppHandle, guid: String, shot: ShotSpec) -> Result<ShotResult, String> {
    blocking(move || {
        let catalog = wait(&app)?;
        let assets = app.sc_render().assets()?;
        catalog
            .ship_hit(&assets, &guid, &shot)
            .map_err(|e| e.to_string())
    })
    .await
}

/// Attack directions on one of a ship's components, shallowest first.
#[tauri::command]
#[specta::specta]
pub async fn ship_exposure(
    app: AppHandle,
    guid: String,
    component: u32,
    samples: u32,
) -> Result<Vec<ExposureDirection>, String> {
    blocking(move || {
        let catalog = wait(&app)?;
        let assets = app.sc_render().assets()?;
        catalog
            .ship_exposure(&assets, &guid, component, samples)
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
        let json = read_dump(&app, path)?;
        wait(&app)?.inventory(&json).map_err(|e| e.to_string())
    })
    .await
}

/// The install's saved characters (`.chf`), newest first.
#[tauri::command]
#[specta::specta]
pub async fn list_character_files() -> Result<Vec<lab_catalog::CharacterFile>, String> {
    blocking(|| lab_catalog::character_files().map_err(|e| e.to_string())).await
}

/// A saved character's colour overrides (hair and brow dye, eye and skin).
#[tauri::command]
#[specta::specta]
pub async fn character_look(path: String) -> Result<lab_catalog::CharacterLook, String> {
    blocking(move || lab_catalog::character_look(path).map_err(|e| e.to_string())).await
}

/// The player's character (actor, body, everything worn) from the same
/// dump; `None` when it has no player stash.
#[tauri::command]
#[specta::specta]
pub async fn load_character(
    app: AppHandle,
    path: Option<String>,
) -> Result<Option<InventoryNode>, String> {
    blocking(move || {
        let json = read_dump(&app, path)?;
        wait(&app)?.character(&json).map_err(|e| e.to_string())
    })
    .await
}

fn read_dump(app: &AppHandle, path: Option<String>) -> Result<String, String> {
    let path = match path {
        Some(p) => p.into(),
        None => app
            .path()
            .data_dir()
            .map_err(|e| e.to_string())?
            .join("starlume-dev/dossier/inventory.json"),
    };
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}
