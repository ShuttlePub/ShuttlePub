//! Regenerates the checked-in OpenAPI document.
//!
//! Usage: `cargo run --locked -p shuttlepub-server --bin openapi [OUTPUT]`
//! (default output: `openapi/shuttlepub.json` at the workspace root).

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join(server::openapi::GENERATED_PATH)
        });
    std::fs::write(&path, server::openapi::render()?)?;
    println!("wrote {}", path.display());
    Ok(())
}
