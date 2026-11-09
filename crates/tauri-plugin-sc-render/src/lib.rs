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
//! `load_model { path, level? }` → `ModelInfo`, `load_material { path }` →
//! `MaterialInfo`, `load_skeleton { path }` → `BoneInfo[]`,
//! `load_holo { root, request? }` → `HoloInfo` (its buffer is the `model` route
//! with `path` = `HoloInfo.path`).
//! Scheme routes:
//!
//! - `scr://localhost/model?path=…`: the packed buffer of a model loaded
//!   with `load_model` (`…@level` for a `level` above 0); one fetch per load,
//!   then the plugin lets go of it.
//! - `scr://localhost/texture?path=…&kind=color|normal|gloss|normal_gloss&max=1024`:
//!   header (width, height, sRGB flag, level count as `u32` LE), then every
//!   level's RGBA8, largest first, each half the one before.
//! - `scr://localhost/hue_lut`: header (width, height, depth as `u32` LE),
//!   then RGBA8.
//!
//! Every command and route also takes `install`, the
//! `ShipScene.install` the request is for; without it they read the primary
//! install. See [`RenderState::assets_at`].
//!
//! A resident host calls [`RenderState::evict`] when its window goes away.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sc_render::{
    Assets, BoneInfo, HoloInfo, HoloRequest, MaterialInfo, ModelInfo, ScenePart, TextureKind,
};
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
            load_skeleton,
            load_holo
        ])
        .register_asynchronous_uri_scheme_protocol("scr", protocol)
        .setup(|app, _| {
            app.manage(RenderState::default());
            Ok(())
        })
        .build()
}

/// The one install open at a time, the ones the host has named, and the
/// model buffers waiting to be fetched.
#[derive(Default)]
pub struct RenderState {
    opened: Mutex<Option<Opened>>,
    known: Mutex<HashSet<String>>,
    buffers: Mutex<HashMap<String, Pending>>,
    holos: AtomicU64,
}

/// The open install: `None` is the primary one.
struct Opened {
    install: Option<String>,
    assets: Arc<Assets>,
}

/// A loaded model's buffer and how many loads have yet to fetch it.
struct Pending {
    buffer: Arc<Vec<u8>>,
    loads: usize,
}

impl RenderState {
    /// The primary install, opened on first call and again after
    /// [`evict`](Self::evict). A host that reads the DataCore too shares it
    /// from here, and does not keep the `Arc` longer than it needs it.
    pub fn assets(&self) -> Result<Arc<Assets>, String> {
        self.open(None)
    }

    /// The install of the `Data.p4k` at `p4k`, for a host that shows more
    /// than one channel. Only one install stays open: asking for another
    /// closes this one once the loads reading it finish.
    ///
    /// Stamp what is built from it with
    /// [`ShipScene::read_from`](sc_render::ShipScene::read_from). The view
    /// then names this install in everything it asks the plugin for, so a
    /// detail level fetched after a channel switch still reads the archive
    /// its ship came from. The page can only name installs passed here.
    ///
    /// ```no_run
    /// # fn demo<R: tauri::Runtime>(app: &tauri::AppHandle<R>, scene: tauri_plugin_sc_render::sc_render::ShipScene) -> Result<(), String> {
    /// use tauri_plugin_sc_render::ScRenderExt;
    /// let p4k = std::path::Path::new(r"C:\Games\StarCitizen\PTU\Data.p4k");
    /// let assets = app.sc_render().assets_at(p4k)?;
    /// let ship = scene.read_from(&assets);
    /// # let _ = ship;
    /// # Ok(())
    /// # }
    /// ```
    pub fn assets_at(&self, p4k: &Path) -> Result<Arc<Assets>, String> {
        let install = p4k.to_string_lossy().into_owned();
        let assets = self.open(Some(&install))?;
        self.known.lock().unwrap().insert(install);
        Ok(assets)
    }

    /// The install a page request names.
    fn requested(&self, install: Option<&str>) -> Result<Arc<Assets>, String> {
        if let Some(i) = install
            && !self.known.lock().unwrap().contains(i)
        {
            return Err(format!("the host has not opened this install: {i}"));
        }
        self.open(install)
    }

    fn open(&self, install: Option<&str>) -> Result<Arc<Assets>, String> {
        let mut slot = self.opened.lock().unwrap();
        if let Some(o) = slot.as_ref()
            && o.install.as_deref() == install
        {
            return Ok(o.assets.clone());
        }
        // Before the new one opens: two archives never need to be held.
        *slot = None;
        let assets = Arc::new(
            match install {
                Some(p4k) => Assets::open_at(Path::new(p4k)),
                None => Assets::open(),
            }
            .map_err(|e| e.to_string())?,
        );
        *slot = Some(Opened {
            install: install.map(str::to_string),
            assets: assets.clone(),
        });
        Ok(assets)
    }

    /// Let go of the install, its decoded textures and any model buffer not
    /// fetched yet. The next call reopens what it asks for. A load in flight
    /// finishes on the `Arc` it holds; a model fetched after this fails, so
    /// call it when no view is drawing.
    ///
    /// ```no_run
    /// # fn demo<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    /// use tauri_plugin_sc_render::ScRenderExt;
    /// app.sc_render().evict();
    /// # }
    /// ```
    pub fn evict(&self) {
        self.opened.lock().unwrap().take();
        self.buffers.lock().unwrap().clear();
    }

    /// Keep a load's buffer until it is fetched.
    fn store(&self, key: String, buffer: Vec<u8>) {
        let mut buffers = self.buffers.lock().unwrap();
        let pending = buffers.entry(key).or_insert_with(|| Pending {
            buffer: Arc::default(),
            loads: 0,
        });
        pending.buffer = Arc::new(buffer);
        pending.loads += 1;
    }

    /// The buffer of a load, released when the last load has fetched it.
    fn take_buffer(&self, key: &str) -> Option<Vec<u8>> {
        let mut buffers = self.buffers.lock().unwrap();
        let pending = buffers.get_mut(key)?;
        pending.loads -= 1;
        if pending.loads == 0 {
            let buffer = buffers.remove(key)?.buffer;
            return Some(Arc::try_unwrap(buffer).unwrap_or_else(|b| b.as_ref().clone()));
        }
        Some(pending.buffer.as_ref().clone())
    }
}

/// Where a model's buffer waits. The same file in two installs is two buffers.
fn buffer_key(install: Option<&str>, path: &str) -> String {
    let path = path.to_ascii_lowercase();
    match install {
        Some(install) => format!("{install}|{path}"),
        None => path,
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
async fn load_model(
    state: State<'_, RenderState>,
    path: String,
    level: Option<u8>,
    install: Option<String>,
) -> Result<ModelInfo, String> {
    let assets = state.requested(install.as_deref())?;
    let level = level.unwrap_or(0);
    let p = path.clone();
    let model = tauri::async_runtime::spawn_blocking(move || assets.model_level(&p, level))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let served = if level == 0 {
        path
    } else {
        format!("{path}@{level}")
    };
    state.store(buffer_key(install.as_deref(), &served), model.buffer);
    Ok(model.info)
}

/// Build a ship's hologram; its buffer is then served at `scr://localhost/model`.
///
/// With `shared`, on WebView2, it is posted as a shared buffer instead: the
/// custom scheme costs about 7.5 ms per MB there, which is most of a big
/// ship's first open.
#[tauri::command]
async fn load_holo<R: Runtime>(
    webview: tauri::Webview<R>,
    state: State<'_, RenderState>,
    root: ScenePart,
    request: Option<HoloRequest>,
    shared: Option<bool>,
    install: Option<String>,
) -> Result<HoloInfo, String> {
    let assets = state.requested(install.as_deref())?;
    let id = format!("holo:{}", state.holos.fetch_add(1, Ordering::Relaxed));
    let tag = id.clone();
    let shared = shared.unwrap_or(false);
    let (mut info, served) = tauri::async_runtime::spawn_blocking(move || {
        build_holo(
            &webview,
            &assets,
            &root,
            &request.unwrap_or_default(),
            &tag,
            shared,
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    info.path = id;
    if let Some(buffer) = served {
        state.store(info.path.clone(), buffer);
    }
    Ok(info)
}

/// Build the hologram into a shared buffer when asked and possible. Returns
/// the bytes to serve over the scheme when it did not end up shared.
fn build_holo<R: Runtime>(
    webview: &tauri::Webview<R>,
    assets: &Assets,
    root: &ScenePart,
    request: &HoloRequest,
    id: &str,
    shared: bool,
) -> (HoloInfo, Option<Vec<u8>>) {
    let mut fallback = Vec::new();
    let mut memory = None;
    let mut failure = None;
    let (out, held, failed) = (&mut fallback, &mut memory, &mut failure);
    let mut info = assets.holo_into(root, request, move |size| {
        if shared {
            match shared::Memory::new(webview, size) {
                Ok(m) => {
                    // SAFETY: filled before `post` below hands the mapping on.
                    let bytes = unsafe { m.bytes() };
                    *held = Some(m);
                    return bytes;
                }
                Err(e) => *failed = Some(e),
            }
        }
        out.resize(size, 0);
        out.as_mut_slice()
    });
    if let Some(m) = memory {
        match m.post(webview, id) {
            Ok(()) => {
                info.shared = true;
                return (info, None);
            }
            Err((e, bytes)) => {
                info.notes.push(format!(
                    "holo: shared buffer failed ({e}), using the scheme"
                ));
                return (info, Some(bytes));
            }
        }
    }
    if let Some(e) = failure {
        info.notes.push(format!(
            "holo: shared buffer failed ({e}), using the scheme"
        ));
    }
    (info, Some(fallback))
}

/// WebView2 shared buffers: memory the page maps, so a big hologram never
/// crosses the custom scheme. The page releases it with
/// `chrome.webview.releaseBuffer`.
#[cfg(windows)]
mod shared {
    use tauri::Runtime;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE, ICoreWebView2_2, ICoreWebView2_17,
        ICoreWebView2Environment12, ICoreWebView2SharedBuffer,
    };
    use windows_core::{HSTRING, Interface, PCWSTR};

    pub struct Memory {
        buffer: ICoreWebView2SharedBuffer,
        start: *mut u8,
        len: usize,
    }

    // SAFETY: moved between threads only to get back to the webview's thread;
    // the COM object is called and released there. Only the mapping, which is
    // plain process memory, is touched elsewhere.
    unsafe impl Send for Memory {}

    impl Memory {
        /// A shared buffer of `len` bytes, made on the webview's thread.
        pub fn new<R: Runtime>(webview: &tauri::Webview<R>, len: usize) -> Result<Self, String> {
            let (done, result) = std::sync::mpsc::channel();
            webview
                .with_webview(move |w| {
                    // SAFETY: on the webview's thread, as these interfaces require.
                    let made = (|| unsafe {
                        let environment = w
                            .controller()
                            .CoreWebView2()?
                            .cast::<ICoreWebView2_2>()?
                            .Environment()?
                            .cast::<ICoreWebView2Environment12>()?;
                        let buffer = environment.CreateSharedBuffer(len as u64)?;
                        let mut start = std::ptr::null_mut();
                        buffer.Buffer(&mut start)?;
                        Ok(Memory { buffer, start, len })
                    })();
                    let _ = done.send(made.map_err(|e: windows_core::Error| e.to_string()));
                })
                .map_err(|e| e.to_string())?;
            result.recv().map_err(|e| e.to_string())?
        }

        /// The mapped memory.
        ///
        /// # Safety
        ///
        /// The slice must not be used after [`post`](Self::post).
        pub unsafe fn bytes<'a>(&self) -> &'a mut [u8] {
            // SAFETY: the mapping is `len` bytes and lives until `post`.
            unsafe { std::slice::from_raw_parts_mut(self.start, self.len) }
        }

        /// Post it to the page, tagged `{ "scRender": id }`, and let go of
        /// our side. On failure, a copy of the bytes for the scheme.
        pub fn post<R: Runtime>(
            self,
            webview: &tauri::Webview<R>,
            id: &str,
        ) -> Result<(), (String, Vec<u8>)> {
            let tag = HSTRING::from(format!("{{\"scRender\":\"{id}\"}}"));
            let (start, len) = (self.start as usize, self.len);
            // SAFETY: the mapping is still ours, `post` has not run.
            let copy =
                move || unsafe { std::slice::from_raw_parts(start as *const u8, len).to_vec() };
            let (done, result) = std::sync::mpsc::channel();
            let memory = self;
            let tag = SendTag(tag);
            let sent = webview.with_webview(move |w| {
                let (memory, tag) = (memory, tag);
                // SAFETY: on the webview's thread, as these interfaces require.
                let posted = (|| unsafe {
                    w.controller()
                        .CoreWebView2()?
                        .cast::<ICoreWebView2_17>()?
                        .PostSharedBufferToScript(
                            &memory.buffer,
                            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
                            PCWSTR(tag.0.as_ptr()),
                        )
                })();
                match posted {
                    Ok(()) => {
                        // The page holds its own view; ours can go.
                        // SAFETY: as above.
                        let _ = done.send(
                            unsafe { memory.buffer.Close() }
                                .map_err(|e| (e.to_string(), Vec::new())),
                        );
                    }
                    Err(e) => {
                        let bytes = copy();
                        // SAFETY: as above.
                        let _ = unsafe { memory.buffer.Close() };
                        let _ = done.send(Err((e.to_string(), bytes)));
                    }
                }
            });
            sent.map_err(|e| (e.to_string(), Vec::new()))?;
            result.recv().map_err(|e| (e.to_string(), Vec::new()))?
        }
    }

    /// The tag, moved to the webview's thread.
    struct SendTag(HSTRING);
    // SAFETY: an HSTRING is an immutable, reference-counted string.
    unsafe impl Send for SendTag {}
}

#[cfg(not(windows))]
mod shared {
    use tauri::Runtime;

    /// Only WebView2 has shared buffers; `new` always fails here.
    pub struct Memory;

    impl Memory {
        pub fn new<R: Runtime>(_: &tauri::Webview<R>, _: usize) -> Result<Self, String> {
            Err("only WebView2 has shared buffers".into())
        }

        /// # Safety
        ///
        /// Never called: no `Memory` is ever made here.
        pub unsafe fn bytes<'a>(&self) -> &'a mut [u8] {
            unreachable!("no shared memory off Windows")
        }

        pub fn post<R: Runtime>(
            self,
            _: &tauri::Webview<R>,
            _: &str,
        ) -> Result<(), (String, Vec<u8>)> {
            unreachable!("no shared memory off Windows")
        }
    }
}

/// A `.chr` skeleton's bones.
#[tauri::command]
async fn load_skeleton(
    state: State<'_, RenderState>,
    path: String,
    install: Option<String>,
) -> Result<Vec<BoneInfo>, String> {
    let assets = state.requested(install.as_deref())?;
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
    install: Option<String>,
) -> Result<MaterialInfo, String> {
    let assets = state.requested(install.as_deref())?;
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
    let install = query.get("install").map(String::as_str);
    if uri.path() == "/hue_lut" {
        let v = state
            .requested(install)?
            .hue_lut()
            .map_err(|e| e.to_string())?;
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
            .take_buffer(&buffer_key(install, path))
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
                .requested(install)?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_cannot_name_an_install_the_host_did_not_open() {
        let state = RenderState::default();
        let refused = state.requested(Some(r"C:\elsewhere\Data.p4k")).err();
        assert!(refused.is_some_and(|e| e.contains("has not opened")));
    }

    #[test]
    fn the_same_file_in_two_installs_is_two_buffers() {
        let state = RenderState::default();
        state.store(buffer_key(Some("live"), "A.cga"), vec![1]);
        state.store(buffer_key(Some("ptu"), "a.CGA"), vec![2]);
        assert_eq!(
            state.take_buffer(&buffer_key(Some("ptu"), "a.cga")),
            Some(vec![2])
        );
        assert_eq!(
            state.take_buffer(&buffer_key(Some("live"), "a.cga")),
            Some(vec![1])
        );
        assert_eq!(state.take_buffer(&buffer_key(None, "a.cga")), None);
    }
}
