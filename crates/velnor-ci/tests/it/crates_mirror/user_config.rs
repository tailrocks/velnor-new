// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use super::fixture::{TestResult, active_toolchain, mirror_fixture};
use crate::io::Repo;

/// Cargo does not merge `--config` given before the subcommand with `--config`
/// given after it: the later group replaces the earlier one. The wrapper puts the
/// mirror configuration first, so a step such as `cargo doc --config
/// 'build.rustdocflags=...'` dropped the mirror, resolved crates.io, unpacked the
/// crates under another registry directory, and made every dependency stale.
#[test]
fn user_config_after_the_subcommand_keeps_the_mirror() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        fetch_with(repo, wrapper, config, 0, |manifest| {
            [
                "fetch",
                "--manifest-path",
                manifest,
                "--config",
                "net.retry=0",
            ]
        })?;
        fetch_with(repo, wrapper, config, 1, |manifest| {
            ["fetch", "--config=net.retry=0", "--manifest-path", manifest]
        })
    })
}

fn fetch_with<const N: usize>(
    repo: &Repo,
    wrapper: &Path,
    config: &Path,
    index: usize,
    arguments: impl FnOnce(&str) -> [&str; N],
) -> TestResult {
    let root = repo.root();
    let project = format!("user-config-{index}");
    repo.write_unstaged(
        &format!("{project}/Cargo.toml"),
        "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n",
    );
    repo.write_unstaged(&format!("{project}/src/lib.rs"), "");
    let manifest = root.join(&project).join("Cargo.toml");
    let manifest = manifest.to_str().ok_or("manifest path")?;
    let home = root.join(format!("user-config-home-{index}"));
    let output = Command::new(wrapper)
        .arg(format!("+{}", active_toolchain()?))
        .args(arguments(manifest))
        .current_dir(root.join(&project))
        .env("VELNOR_CARGO_REAL", "/usr/bin/false")
        .env("CARGO_HOME", &home)
        .env("VELNOR_CARGO_CONFIG", config)
        .env("VELNOR_CARGO_WRAPPER", wrapper)
        .env("CARGO_NET_OFFLINE", "false")
        .env("CARGO_HTTP_PROXY", "http://127.0.0.1:1")
        .env("NO_PROXY", "127.0.0.1")
        .env("CARGO_NET_RETRY", "0")
        .env(
            "CARGO_REGISTRIES_CRATES_IO_INDEX",
            "sparse+http://127.0.0.1:1/index/",
        )
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert!(fs::metadata(home.join("registry/src")).is_ok_and(|meta| meta.is_dir()));
    Ok(())
}
