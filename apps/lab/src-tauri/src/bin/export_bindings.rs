//! Regenerate `src/lib/bindings.ts` without launching the app.
//!
//!     cargo run --features bindgen --bin export-bindings
//!
//! `run()` does this too on every debug start, but a CI check ("are the
//! committed bindings current?") shouldn't need a window and a GPU.

use specta_typescript::Typescript;

fn main() {
    let path = sc_item_render_lib::bindings_path();
    sc_item_render_lib::specta_builder()
        .export(Typescript::default(), &path)
        .expect("failed to export typescript bindings");
    println!("wrote {}", path.display());
}
