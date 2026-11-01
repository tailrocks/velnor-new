// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::process::run_bounded;

use super::BaseRef;

const GIT_OUTPUT_CAP: usize = 512 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(20);
const SHA1_HEX_LEN: usize = 40;
const SHA256_HEX_LEN: usize = 64;

#[derive(Debug)]
pub(super) enum Blob {
    Present(Vec<u8>),
    Absent,
}

pub(super) fn resolve_commit(root: &Path, base: &BaseRef) -> Result<String, String> {
    let revision = format!("{}^{{commit}}", base.as_str());
    let output = run_git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &revision,
        ],
    )?;
    let oid = String::from_utf8(output)
        .map_err(|_| "rev-parse output is not UTF-8".to_owned())?
        .trim()
        .to_owned();
    if is_object_id(&oid) {
        Ok(oid)
    } else {
        Err("rev-parse output is not an object id".to_owned())
    }
}

pub(super) fn read_blob(root: &Path, oid: &str, path: &str) -> Result<Blob, String> {
    if !is_object_id(oid) {
        return Err("commit id is not an object id".to_owned());
    }
    let listing = run_git(root, &["ls-tree", "-z", "--full-tree", oid, "--", path])?;
    if listing.is_empty() {
        return Ok(Blob::Absent);
    }
    check_regular_blob_entry(&listing, path)?;
    run_git(root, &["cat-file", "blob", &format!("{oid}:{path}")]).map(Blob::Present)
}

fn check_regular_blob_entry(listing: &[u8], path: &str) -> Result<(), String> {
    let text = std::str::from_utf8(listing)
        .map_err(|_| format!("ls-tree entry for {path} is not UTF-8"))?;
    let entry = text
        .strip_suffix('\0')
        .ok_or_else(|| format!("ls-tree entry for {path} is unterminated"))?;
    let (meta, name) = entry
        .split_once('\t')
        .ok_or_else(|| format!("ls-tree entry for {path} is malformed"))?;
    let mut fields = meta.split(' ');
    let regular = matches!(
        (fields.next(), fields.next()),
        (Some("100644" | "100755"), Some("blob"))
    );
    if name == path && regular {
        Ok(())
    } else {
        Err(format!("{path} is not a regular file in the base commit"))
    }
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), SHA1_HEX_LEN | SHA256_HEX_LEN)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn run_git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let verb = args.first().copied().unwrap_or("git");
    let mut command = git_command(root);
    command.args(args);
    let output = run_bounded(&mut command, GIT_OUTPUT_CAP, GIT_TIMEOUT)
        .map_err(|error| format!("git {verb} failed ({error})"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "git {verb} failed ({}): {}",
            output.status,
            stderr.trim().chars().take(200).collect::<String>()
        ))
    }
}

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(path) = env::var_os("PATH") {
        command.env("PATH", path);
    }
    command.arg("-C").arg(root);
    command
}
