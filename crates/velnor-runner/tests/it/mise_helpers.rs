// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

pub(crate) fn pinned_mise_action() -> Result<String> {
    let policy: toml::Table = include_str!("../../../../.velnor/version-policy.toml")
        .parse()
        .context("parse the version policy")?;
    policy["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|action| action["name"].as_str() == Some("jdx/mise-action"))
        .and_then(|action| action["sha"].as_str())
        .map(str::to_owned)
        .context("jdx/mise-action is not pinned in the version policy")
}

pub(crate) fn real_action() -> Result<String> {
    let path = manifest("../../actions/setup-velnor/action.yml");
    std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
}

pub(crate) fn manifest(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

pub(crate) fn job_fixture(name: &str) -> PathBuf {
    manifest(&format!("tests/fixtures/jobs/{name}.json"))
}

pub(crate) fn git(directory: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .with_context(|| format!("run git {args:?}"))?;
    ensure!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(crate) fn sha256_hex(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        }))
}
