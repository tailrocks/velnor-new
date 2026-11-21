// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;

use crate::facts::{TestResult, path};
use crate::private_sources::{SECTION, private_repo};

const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef\"\n";
const MIRROR: &[u8] = b"[source.mirror]\nregistry = \"https://registry.example.invalid/\"\n";
const HARMLESS: &[u8] = b"[build]\njobs = 2\n";

fn saves_base_caches(configs: &[(&str, &[u8])]) -> Result<bool, Box<dyn std::error::Error>> {
    let mut repo = private_repo(&["Cargo.toml"], SECTION, "0.1.111")?;
    repo.files
        .insert(path("Cargo.lock")?, GIT_LOCK.as_bytes().to_vec());
    for (name, bytes) in configs {
        repo.files.insert(path(name)?, bytes.to_vec());
    }
    crate::facts::lock_pinned_tools(&mut repo)?;
    let (mut files, _) = expected_with_notes(&repo)?;
    let text = files
        .remove(&path(".github/workflows/rust-ci.yml")?)
        .ok_or("no workflow")?;
    Ok(text.contains("Save the target cache"))
}

#[test]
fn a_config_toml_that_cargo_ignores_beside_a_config_does_not_refuse_the_private_caches()
-> TestResult {
    assert!(saves_base_caches(&[
        (".cargo/config", HARMLESS),
        (".cargo/config.toml", MIRROR),
    ])?);
    Ok(())
}

#[test]
fn the_config_cargo_reads_still_refuses_the_private_caches() -> TestResult {
    assert!(!saves_base_caches(&[(".cargo/config.toml", MIRROR)])?);
    assert!(!saves_base_caches(&[(".cargo/config", MIRROR)])?);
    assert!(!saves_base_caches(&[
        (".cargo/config", MIRROR),
        (".cargo/config.toml", HARMLESS),
    ])?);
    Ok(())
}
