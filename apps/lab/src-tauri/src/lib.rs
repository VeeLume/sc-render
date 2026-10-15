mod catalog;
mod commands;
mod render;
mod settings;
mod state;
mod store;

pub use state::AppState;

// Only used by the debug-only bindings export; gate the import so release
// builds don't warn about it.
#[cfg(all(debug_assertions, desktop))]
use specta_typescript::Typescript;
use tauri::Manager;
use tauri_specta::{collect_commands, Builder};

/// Where the generated TypeScript IPC bindings land. Anchored to the crate
/// directory, not the working directory — `tauri dev` and `cargo run` from the
/// workspace root have different cwds, and a relative path silently writes the
/// file into the wrong tree.
pub fn bindings_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/bindings.ts")
}

/// The single definition of the IPC surface. A command missing from this list
/// exists in Rust but not on the frontend.
///
/// Shared with `src/bin/export_bindings.rs` so CI can regenerate the bindings
/// without launching a window.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::settings_get,
        commands::settings_save,
        render::load_model,
        render::load_material,
        render::load_skeleton,
        render::save_snapshot,
        render::save_log,
        catalog::search_items,
        catalog::item_scene,
        catalog::load_inventory,
        catalog::load_character,
        catalog::list_character_files,
        catalog::character_look,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();

    // Writes into the source tree, which doesn't exist on a device — running
    // this on mobile panics and kills the app.
    #[cfg(all(debug_assertions, desktop))]
    builder
        .export(Typescript::default(), bindings_path())
        .expect("failed to export typescript bindings");

    #[allow(unused_mut)]
    let mut app = tauri::Builder::default();

    app.plugin(tauri_plugin_opener::init())
        .manage(render::RenderState::default())
        .manage(catalog::CatalogState::default())
        .register_asynchronous_uri_scheme_protocol("scr", render::protocol)
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            catalog::start_loading(app.handle());

            #[cfg(desktop)]
            {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
                app.handle().plugin(tauri_plugin_process::init())?;
            }

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let state = AppState::load(&data_dir);

            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
