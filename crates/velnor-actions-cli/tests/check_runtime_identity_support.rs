// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into())
    }
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into())
    }
}
