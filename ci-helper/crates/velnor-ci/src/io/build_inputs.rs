// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use velnor_gen::io::read_files;
use velnor_model::RepoPath;
use velnor_rust::{repo_profile, rustc_release};

use crate::error::CiError;

const CONFIG_PATHSPECS: [&str; 2] = [":(glob)**/.cargo/config", ":(glob)**/.cargo/config.toml"];
const OS_RELEASE: &str = "/etc/os-release";

pub(super) fn repository_profile(root: &Path) -> Result<String, CiError> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
        ])
        .args(CONFIG_PATHSPECS)
        .current_dir(root)
        .output()
        .map_err(|e| CiError::new(format!("cannot run git: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| {
            std::str::from_utf8(name)
                .map_err(|_| CiError::new("a cargo config path is not UTF-8".to_owned()))
                .and_then(|name| RepoPath::new(name).map_err(|e| CiError::new(e.to_string())))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let files = read_files(root, &paths).map_err(|e| CiError::new(e.to_string()))?;
    repo_profile(&files).map_err(|e| CiError::new(e.to_string()))
}

pub(super) fn release_of(verbose: &str) -> Result<(u32, u32), CiError> {
    rustc_release(verbose).ok_or_else(|| CiError::new("rustc -vV reports no release".to_owned()))
}

pub(super) fn image_identity() -> String {
    let image_os = std::env::var("ImageOS").unwrap_or_default();
    let os_release = std::fs::read_to_string(OS_RELEASE).unwrap_or_default();
    let field = |name: &str| {
        os_release
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .map_or_else(String::new, |value| value.trim_matches('"').to_owned())
    };
    format!("{image_os}|{}|{}", field("ID"), field("VERSION_ID"))
}
