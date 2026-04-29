fn main() {
    tauri_build::build();

    // Export TypeScript bindings to frontend
    // This runs when the crate is compiled
    #[cfg(debug_assertions)]
    export_ts_bindings();
}

#[cfg(debug_assertions)]
fn export_ts_bindings() {
    use std::path::PathBuf;

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bindings_dir = crate_dir
        .parent()
        .unwrap()
        .join("src")
        .join("lib")
        .join("bindings");

    // Set the export directory for ts-rs
    std::env::set_var("TS_RS_EXPORT_DIR", bindings_dir.to_str().unwrap());

    // The actual export happens via #[ts(export)] and cargo test export_bindings
    // This build script just ensures the directory exists
    if let Err(e) = std::fs::create_dir_all(&bindings_dir) {
        eprintln!("Warning: Could not create bindings directory: {}", e);
    }
}
