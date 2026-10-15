const COMMANDS: &[&str] = &["load_model", "load_material", "load_skeleton"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
