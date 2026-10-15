//! sc-render for a Tauri webview: commands for the descriptions, and the
//! `scr` URI scheme for the heavy bytes (model buffers, decoded textures),
//! so they skip JSON. The `@veelume/sc-render/tauri` source reads both.
//!
//! ```no_run
//! tauri::Builder::default()
//!     .plugin(tauri_plugin_sc_render::init())
//! #   ;
//! ```
//!
//! Capabilities: `sc-render:default`. Commands (`plugin:sc-render|…`):
//! `load_model { path }` → `ModelInfo`, `load_material { path }` →
//! `MaterialInfo`, `load_skeleton { path }` → `BoneInfo[]`. Scheme routes:
//!
//! - `scr://localhost/model?path=…`: the packed buffer of a model loaded
//!   with `load_model`.
//! - `scr://localhost/texture?path=…&kind=color|normal|gloss|normal_gloss&max=1024`:
//!   header (width, height, sRGB flag, level count as `u32` LE), then every
//!   level's RGBA8, largest first, each half the one before.
//! - `scr://localhost/hue_lut`: header (width, height, depth as `u32` LE),
//!   then RGBA8.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use sc_render::{Assets, BoneInfo, MaterialInfo, ModelInfo, TextureKind};
use tauri::http::{Request, Response, StatusCode};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime, State, UriSchemeContext, UriSchemeResponder};

pub use sc_render;

/// The plugin: registers the commands, the `scr` scheme and [`RenderState`].
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("sc-render")
        .invoke_handler(tauri::generate_handler![
            load_model,
            load_material,
            load_skeleton
        ])
        .register_asynchronous_uri_scheme_protocol("scr", protocol)
        .setup(|app, _| {
            app.manage(RenderState::default());
            Ok(())
        })
        .build()
}

/// The install the renderer reads, opened on first use, and the model
/// buffers waiting to be fetched.
#[derive(Default)]
pub struct RenderState {
    assets: OnceLock<Arc<Assets>>,
    buffers: Mutex<HashMap<String, Arc<Vec<u8>>>>,
}

impl RenderState {
    /// The primary install, opened on first call. A host that reads the
    /// DataCore too shares it from here.
    pub fn assets(&self) -> Result<Arc<Assets>, String> {
        if let Some(a) = self.assets.get() {
            return Ok(a.clone());
        }
        let opened = Arc::new(Assets::open().map_err(|e| e.to_string())?);
        Ok(self.assets.get_or_init(|| opened).clone())
    }
}

/// [`RenderState`] from any app handle, window or webview.
pub trait ScRenderExt<R: Runtime> {
    fn sc_render(&self) -> State<'_, RenderState>;
}

impl<R: Runtime, T: Manager<R>> ScRenderExt<R> for T {
    fn sc_render(&self) -> State<'_, RenderState> {
        self.state::<RenderState>()
    }
}

/// Load a geometry file; its buffer is then served at `scr://localhost/model`.
#[tauri::command]
async fn load_model(state: State<'_, RenderState>, path: String) -> Result<ModelInfo, String> {
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

/// A `.chr` skeleton's bones.
#[tauri::command]
async fn load_skeleton(
    state: State<'_, RenderState>,
    path: String,
) -> Result<Vec<BoneInfo>, String> {
    let assets = state.assets()?;
    tauri::async_runtime::spawn_blocking(move || assets.skeleton(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// A `.mtl` file (extension optional).
#[tauri::command]
async fn load_material(
    state: State<'_, RenderState>,
    path: String,
) -> Result<MaterialInfo, String> {
    let assets = state.assets()?;
    tauri::async_runtime::spawn_blocking(move || assets.material(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// The `scr` scheme handler; decoding runs off the webview thread.
fn protocol<R: Runtime>(
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
    if uri.path() == "/hue_lut" {
        let v = state.assets()?.hue_lut().map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(12 + v.data.len());
        for n in [v.width, v.height, v.depth] {
            out.extend(n.to_le_bytes());
        }
        out.extend_from_slice(&v.data);
        return Ok(out);
    }
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
                Some("normal_gloss") => TextureKind::NormalGloss,
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
            let len = t.data.len() + t.mips.iter().map(|m| m.data.len()).sum::<usize>();
            let mut out = Vec::with_capacity(16 + len);
            out.extend(t.width.to_le_bytes());
            out.extend(t.height.to_le_bytes());
            out.extend(u32::from(t.srgb).to_le_bytes());
            out.extend((1 + t.mips.len() as u32).to_le_bytes());
            out.extend_from_slice(&t.data);
            for m in &t.mips {
                out.extend_from_slice(&m.data);
            }
            Ok(out)
        }
        other => Err(format!("unknown route {other}")),
    }
}
