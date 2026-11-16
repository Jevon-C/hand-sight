//! Regenerate `docs/api/openapi.json` from the shared types.
//!
//! Run from the repository root: `cargo run -p axonhand-api --example write_openapi`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/api/openapi.json");
    std::fs::write(path, axonhand_api::openapi_json()?)?;
    println!("wrote {path}");
    Ok(())
}
