// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io, path::PathBuf, process::Command};

fn pinned_gh_binary() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = std::env::var_os("VELNOR_GH_BINARY") {
        return Ok(PathBuf::from(path));
    }
    let output = Command::new("mise")
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "which",
            "gh",
            "--tool",
            "gh@2.103.3",
        ])
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_OFFLINE", "1")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("qualified pinned gh is unavailable").into());
    }
    let path = String::from_utf8(output.stdout)?.trim().to_owned();
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(io::Error::other("qualified pinned gh path is not absolute").into());
    }
    Ok(path)
}

#[test]
fn pinned_gh_verifies_the_exact_three_subject_preview_statement() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output =
        Command::new("python3")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("VELNOR_GH_BINARY", pinned_gh_binary()?)
            .arg(root.join(
                "crates/velnor-actions-workflow-renderer/tests/source_preview_dsse_scenarios.py",
            ))
            .arg(root)
            .output()?;
    assert!(
        output.status.success(),
        "pinned gh three-subject verification failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
