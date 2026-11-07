// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::{Read, Seek, Write};
use std::path::Path;

pub(super) fn verify_real_archive_cache(assets: &Path) -> Result<(), Box<dyn Error>> {
    let home = tempfile::tempdir()?;
    let archive_cache = home.path().join("archives");
    std::fs::create_dir(&archive_cache)?;
    for entry in std::fs::read_dir(assets.join("rust-archives"))? {
        let entry = entry?;
        std::fs::copy(entry.path(), archive_cache.join(entry.file_name()))?;
    }
    let channel = std::fs::read_to_string(assets.join("channel.toml"))?;
    let section = channel
        .split("[pkg.cargo.target.x86_64-unknown-linux-gnu]\n")
        .nth(1)
        .ok_or("missing Cargo channel section")?
        .split("\n[")
        .next()
        .ok_or("missing Cargo channel section")?;
    let cargo_hash = section
        .lines()
        .find_map(|line| {
            line.strip_prefix("xz_hash = ")
                .and_then(|value| value.strip_prefix('"'))
                .and_then(|value| value.strip_suffix('"'))
        })
        .ok_or("missing Cargo archive hash")?;
    let cargo_archive = archive_cache.join(format!("{cargo_hash}.tar.xz"));
    let verify = || -> Result<std::process::Output, Box<dyn Error>> {
        let runner = tempfile::tempdir()?;
        let cache = runner.path().join("velnor/rust-verified");
        std::fs::create_dir_all(&cache)?;
        std::fs::copy(
            assets.join("channel.toml"),
            cache.join("channel-rust-1.99.1.toml"),
        )?;
        let argv = velnor_actions_workflow_renderer::verifier_builders::verify_rust_argv(
            &velnor_actions_mise::ToolCatalog::pinned(),
        );
        Ok(std::process::Command::new("/bin/sh")
            .args(&argv[1..])
            .env_clear()
            .env("HOME", runner.path())
            .env("RUNNER_TEMP", runner.path())
            .env(
                "RUSTUP_HOME",
                std::env::var_os("RUSTUP_HOME").ok_or("missing RUSTUP_HOME")?,
            )
            .env("CARGO_HOME", home.path())
            .env("VELNOR_RUST_ARCHIVE_CACHE", &archive_cache)
            .output()?)
    };
    let first = verify()?;
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(cargo_archive)?;
    let mut byte = [0];
    file.read_exact(&mut byte)?;
    file.rewind()?;
    file.write_all(&[byte[0] ^ 1])?;
    file.sync_all()?;
    let bad = verify()?;
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("rust_component_tarball_digest_mismatch")
    );
    file.rewind()?;
    file.write_all(&byte)?;
    file.sync_all()?;
    let restored = verify()?;
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    Ok(())
}
