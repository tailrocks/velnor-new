#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#[path = "build_support/archive_guard_inputs.rs"]
mod archive_guard_inputs;
use std::error::Error;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or("Cargo did not provide CARGO_MANIFEST_DIR")?;
    if !manifest.is_absolute() {
        return Err("Cargo provided a non-absolute CARGO_MANIFEST_DIR".into());
    }
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root is missing")?;
    let (fingerprint, watched) = archive_guard_inputs::fingerprint(root)?;
    for source in watched {
        println!("cargo:rerun-if-changed={}", source.display());
    }
    println!("cargo:rustc-env=VELNOR_ARCHIVE_GUARD_FINGERPRINT={fingerprint}");
    Ok(())
}
