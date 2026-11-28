// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::{Command, Stdio};

const LS: &str = "/bin/ls";
const WRITING: [&str; 8] = [
    "write",
    "append",
    "add_file",
    "add_subdirectory",
    "delete_child",
    "delete",
    "writesecurity",
    "chown",
];

pub(super) fn grants_write(path: &Path) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let listing = Command::new(LS)
        .args(["-lde", "--"])
        .arg(path)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match listing {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(allows_write),
        _ => true,
    }
}

fn allows_write(line: &str) -> bool {
    let Some((index, entry)) = line.trim_start().split_once(": ") else {
        return false;
    };
    if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let Some((head, permissions)) = entry.rsplit_once(' ') else {
        return false;
    };
    head.ends_with(" allow") && permissions.split(',').any(|name| WRITING.contains(&name))
}
