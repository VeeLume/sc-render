//! Render inputs for the webview: typed commands for descriptions, and the
//! `scr` URI scheme for the heavy bytes (model buffers, decoded textures),
//! so they skip JSON.
//!
//! - `scr://localhost/model?path=…` — the packed buffer of a model loaded
//!   with [`load_model`].
//! - `scr://localhost/texture?path=…&kind=color|normal|gloss&max=1024` —
//!   8-byte header (width, height as `u32` LE) then RGBA8 rows.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use render_assets::{Assets, MaterialInfo, ModelInfo, TextureKind};
use tauri::http::{Request, Response, StatusCode};
use tauri::{Manager, State, UriSchemeContext, UriSchemeResponder};

#[derive(Default)]
pub struct RenderState {
    assets: OnceLock<Arc<Assets>>,
    buffers: Mutex<HashMap<String, Arc<Vec<u8>>>>,
}

impl RenderState {
    fn assets(&self) -> Result<Arc<Assets>, String> {
        if let Some(a) = self.assets.get() {
            return Ok(a.clone());
        }
        let opened = Arc::new(Assets::open().map_err(|e| e.to_string())?);
        Ok(self.assets.get_or_init(|| opened).clone())
    }
}

/// Load a geometry file; its buffer is then served at `scr://localhost/model`.
#[tauri::command]
#[specta::specta]
pub async fn load_model(state: State<'_, RenderState>, path: String) -> Result<ModelInfo, String> {
    let assets = state.assets()?;
    let p = path.clone();
    let model = tauri::async_runtime::spawn_blocking(move || assets.model(&p))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    state
        .buffers
        .lock()
        .unwrap()
        .insert(path.to_ascii_lowercase(), Arc::new(model.buffer));
    Ok(model.info)
}

/// A `.mtl` file (extension optional).
#[tauri::command]
#[specta::specta]
pub async fn load_material(
    state: State<'_, RenderState>,
    path: String,
) -> Result<MaterialInfo, String> {
    let assets = state.assets()?;
    tauri::async_runtime::spawn_blocking(move || assets.material(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

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

/// The `scr` scheme handler; decoding runs off the webview thread.
pub fn protocol<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    std::thread::spawn(move || {
        let state = app.state::<RenderState>();
        let response = match serve(&state, &request) {
            Ok(body) => Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/octet-stream")
                .header("Access-Control-Allow-Origin", "*")
                .body(body),
            Err(e) => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Access-Control-Allow-Origin", "*")
                .body(e.into_bytes()),
        };
        responder.respond(response.expect("valid response"));
    });
}

fn serve(state: &RenderState, request: &Request<Vec<u8>>) -> Result<Vec<u8>, String> {
    let uri = request.uri();
    let query: HashMap<String, String> = uri
        .query()
        .unwrap_or_default()
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| {
            (
                k.to_string(),
                urlencoding::decode(v)
                    .map(|s| s.into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let path = query.get("path").ok_or("missing path")?;
    match uri.path() {
        "/model" => state
            .buffers
            .lock()
            .unwrap()
            .get(&path.to_ascii_lowercase())
            .map(|b| b.as_ref().clone())
            .ok_or_else(|| format!("model not loaded: {path}")),
        "/texture" => {
            let kind = match query.get("kind").map(String::as_str) {
                Some("normal") => TextureKind::Normal,
                Some("gloss") => TextureKind::Gloss,
                _ => TextureKind::Color,
            };
            let max = query
                .get("max")
                .and_then(|m| m.parse().ok())
                .unwrap_or(1024);
            let t = state
                .assets()?
                .texture(path, kind, max)
                .map_err(|e| e.to_string())?;
            let mut out = Vec::with_capacity(8 + t.data.len());
            out.extend(t.width.to_le_bytes());
            out.extend(t.height.to_le_bytes());
            out.extend_from_slice(&t.data);
            Ok(out)
        }
        other => Err(format!("unknown route {other}")),
    }
}
