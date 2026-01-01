//! Embed the exact native archive guard source fingerprint in the CLI binary.

#[path = "build_support/archive_guard_inputs.rs"]
mod archive_guard_inputs;
use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("workspace root is missing")?;
    let (fingerprint, watched) = archive_guard_inputs::fingerprint(root)?;
    for source in watched {
        println!("cargo:rerun-if-changed={}", source.display());
    }
    println!("cargo:rustc-env=VELNOR_ARCHIVE_GUARD_FINGERPRINT={fingerprint}");
    Ok(())
}
