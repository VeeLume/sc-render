//! Write the TypeScript types the renderer package consumes
//! (`packages/sc-render/src/types.ts`) from this crate's.
//!
//! ```bash
//! cargo run -p sc-render --features specta --example export_types
//! ```

use sc_render::{BoneInfo, MaterialInfo, ModelInfo, PartLook, SceneItem, TextureKind};
use specta::TypeCollection;
use specta_typescript::Typescript;

fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/sc-render/src/types.ts");
    let mut types = TypeCollection::default();
    types
        .register::<ModelInfo>()
        .register::<MaterialInfo>()
        .register::<BoneInfo>()
        .register::<TextureKind>()
        .register::<SceneItem>()
        .register::<PartLook>();
    Typescript::default()
        .export_to(&path, &types)
        .expect("failed to export types");
    println!("wrote {}", path.display());
}
