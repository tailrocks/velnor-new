// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#[path = "build_support/archive_guard_inputs.rs"]
mod archive_guard_inputs;
#[path = "build_support/archive_guard_watch.rs"]
mod archive_guard_watch;
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
    let (fingerprint, watched, absent) = archive_guard_inputs::fingerprint(root)?;
    let out_dir = std::env::var_os("OUT_DIR").map(PathBuf::from);
    let plan = archive_guard_watch::plan(watched, absent, out_dir.as_deref());
    for source in &plan.rerun {
        println!("cargo:rerun-if-changed={}", source.display());
    }
    if !plan.unwatched.is_empty() {
        println!(
            "cargo:warning=archive guard: {} cannot be watched because cargo output {} is reachable from the checkout; a binary built before they appear refuses to print its fingerprint",
            plan.unwatched.join(" "),
            out_dir
                .as_deref()
                .map_or_else(String::new, |out| out.display().to_string())
        );
    }
    let unwatched_root = if plan.unwatched.is_empty() {
        ""
    } else {
        root.to_str().ok_or("workspace root is not UTF-8")?
    };
    println!("cargo:rustc-env=VELNOR_ARCHIVE_GUARD_FINGERPRINT={fingerprint}");
    println!("cargo:rustc-env=VELNOR_ARCHIVE_GUARD_ROOT={unwatched_root}");
    println!(
        "cargo:rustc-env=VELNOR_ARCHIVE_GUARD_UNWATCHED={}",
        plan.unwatched.join(" ")
    );
    Ok(())
}
