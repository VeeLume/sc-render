//! List the install's saved characters (`.chf`) and print the colour
//! overrides of one (the newest, or the given file name).
//!
//! ```bash
//! cargo run -p lab-catalog --release --example character_look -- [name]
//! ```

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let files = lab_catalog::character_files()?;
    for f in &files {
        println!("{:30} {}", f.name, f.path);
    }
    let wanted = std::env::args().nth(1);
    let Some(file) = files.iter().find(|f| {
        wanted
            .as_ref()
            .is_none_or(|w| f.name.eq_ignore_ascii_case(w))
    }) else {
        return Ok(());
    };
    println!("\n== {}", file.name);
    for p in lab_catalog::character_look(&file.path)?.parts {
        println!("{} {}", p.material, p.item);
        println!("  params {:?}", p.params);
        println!("  colors {:?}", p.colors);
    }
    Ok(())
}
