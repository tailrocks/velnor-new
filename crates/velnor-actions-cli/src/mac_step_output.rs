// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, io::Write, os::unix::fs::MetadataExt, path::PathBuf};
use velnor_actions_orchestrator::{OrchestratorError, VerifiedMacAttestation};

fn invalid() -> OrchestratorError {
    OrchestratorError::Internal {
        problem: "mac_verification_output_invalid".to_owned(),
    }
}

pub(super) fn write(value: &VerifiedMacAttestation) -> Result<(), OrchestratorError> {
    let pairs = [
        ("manifest_digest", &value.manifest_sha256),
        ("checksums_digest", &value.checksums_sha256),
        ("license_digest", &value.license_sha256),
        ("notice_digest", &value.notice_sha256),
    ];
    if pairs.iter().any(|(_, digest)| !valid_digest(digest)) {
        return Err(invalid());
    }
    let path = PathBuf::from(std::env::var_os("GITHUB_OUTPUT").ok_or_else(invalid)?);
    if !path.is_absolute() {
        return Err(invalid());
    }
    let before = fs::symlink_metadata(&path).map_err(|_| invalid())?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.nlink() != 1
        || before.uid() != rustix::process::geteuid().as_raw()
        || before.len() != 0
    {
        return Err(invalid());
    }
    let fd = rustix::fs::open(
        &path,
        rustix::fs::OFlags::RDWR
            | rustix::fs::OFlags::APPEND
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| invalid())?;
    let mut file = fs::File::from(fd);
    file.try_lock().map_err(|_| invalid())?;
    let opened = file.metadata().map_err(|_| invalid())?;
    if opened.dev() != before.dev() || opened.ino() != before.ino() || opened.len() != before.len()
    {
        return Err(invalid());
    }
    let mut output = String::new();
    for (name, digest) in pairs {
        use std::fmt::Write as _;
        writeln!(output, "{name}={digest}").map_err(|_| invalid())?;
    }
    file.write_all(output.as_bytes()).map_err(|_| invalid())?;
    let after = fs::symlink_metadata(&path).map_err(|_| invalid())?;
    if !after.is_file()
        || after.dev() != opened.dev()
        || after.ino() != opened.ino()
        || after.len() != output.len() as u64
    {
        return Err(invalid());
    }
    Ok(())
}
fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|v| {
        v.len() == 64
            && v.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}
