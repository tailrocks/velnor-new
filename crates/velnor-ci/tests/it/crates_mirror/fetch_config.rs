// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;

use super::fixture::{TestResult, active_toolchain, mirror_fixture};
use crate::fetch::{TOKEN, serve};
use crate::io::Repo;

/// The token fetch passes `--config net.offline=false` after `fetch`. Cargo does
/// not merge that group with a `--config` given before the subcommand, so the
/// mirror came first and was dropped: the fetch resolved crates.io, unpacked the
/// crates under another registry directory, and made every dependency stale.
#[test]
fn the_token_fetch_resolves_through_the_mirror() -> TestResult {
    mirror_fixture(|repo, _wrapper, config| {
        for index in 0..2 {
            fetch(repo, config, index)?;
        }
        Ok(())
    })
}

fn fetch(repo: &Repo, config: &Path, index: usize) -> TestResult {
    let root = &fs::canonicalize(repo.root())?;
    let project = format!("token-fetch-{index}");
    let checksum = velnor_rust::sha256_hex(&fs::read(root.join("probe.crate"))?);
    repo.write_unstaged(
        &format!("{project}/Cargo.toml"),
        "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n",
    );
    repo.write_unstaged(&format!("{project}/src/lib.rs"), "");
    repo.write_unstaged(
        &format!("{project}/Cargo.lock"),
        &format!(
            "version = 4\n\n[[package]]\nname = \"consumer\"\nversion = \"1.0.0\"\ndependencies = [\"probe\"]\n\n[[package]]\nname = \"probe\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{checksum}\"\n"
        ),
    );
    let toolchain = active_toolchain()?;
    let trusted = root.join("trusted/bin");
    fs::create_dir_all(&trusted)?;
    for tool in ["cargo", "rustc"] {
        if !trusted.join(tool).exists() {
            symlink(
                Path::new(&toolchain).join("bin").join(tool),
                trusted.join(tool),
            )?;
        }
    }
    let home = trusted
        .parent()
        .ok_or("trusted")?
        .join(format!("home-{index}"));
    fs::create_dir_all(&home)?;
    let api = serve(204)?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["fetch", "--trusted-dir"])
        .arg(root.join("trusted"))
        .arg("--manifest-path")
        .arg(root.join(&project).join("Cargo.toml"))
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", trusted.display()))
        .env("VELNOR_API_URL", &api.url)
        .env("VELNOR_PRIVATE_TOKEN", TOKEN)
        .env("VELNOR_CARGO_CONFIG", config)
        .env("CARGO_HOME", &home)
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env("NO_PROXY", "127.0.0.1")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert!(fs::metadata(home.join("registry/src")).is_ok_and(|meta| meta.is_dir()));
    Ok(())
}
