// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{fs, io::Read as _, path::Path, process::Command, time::Duration};
use velnor_actions_mise::release_git_pin::{
    GIT_PACKAGE_SHA256, GIT_PACKAGE_URL, VerifiedGit, verify_bundle,
};

pub(crate) fn acquire(runner_temp: &Path) -> Result<VerifiedGit, OrchestratorError> {
    let invalid = || internal("release_git_bundle_invalid");
    let velnor = runner_temp.join("velnor");
    if !runner_temp.is_absolute()
        || runner_temp.canonicalize().map_err(|_| invalid())? != runner_temp
        || velnor.canonicalize().map_err(|_| invalid())? != velnor
        || !velnor.is_dir()
    {
        return Err(invalid());
    }
    let root = velnor.join("git-bundle");
    if root.exists() {
        return ready(&root);
    }
    let scratch = tempfile::Builder::new()
        .prefix("git-acquire-")
        .tempdir_in(&velnor)
        .map_err(|_| invalid())?;
    let mut response = crate::api::github_evidence_http::agent()?
        .get(GIT_PACKAGE_URL)
        .config()
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .call()
        .map_err(|_| invalid())?;
    if response.status().as_u16() != 200 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    verify_package(&bytes, GIT_PACKAGE_SHA256)?;
    let package = scratch.path().join("git.deb");
    fs::write(&package, bytes).map_err(|_| invalid())?;
    let extracted = scratch.path().join("extracted");
    let status = Command::new("/usr/bin/dpkg-deb")
        .args(["-x"])
        .arg(&package)
        .arg(&extracted)
        .env_clear()
        .status()
        .map_err(|_| invalid())?;
    if !status.success() {
        return Err(invalid());
    }
    let staged = scratch.path().join("bundle");
    fs::create_dir(&staged).map_err(|_| invalid())?;
    fs::copy(extracted.join("usr/bin/git"), staged.join("git")).map_err(|_| invalid())?;
    fs::copy(
        extracted.join("usr/lib/git-core/git-remote-http"),
        staged.join("git-remote-http"),
    )
    .map_err(|_| invalid())?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("git-remote-http", staged.join("git-remote-https"))
        .map_err(|_| invalid())?;
    ready(&staged)?;
    install_noreplace(&staged, &root)?;
    ready(&root)
}

#[cfg(target_os = "linux")]
fn install_noreplace(staged: &Path, root: &Path) -> Result<(), OrchestratorError> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staged,
        rustix::fs::CWD,
        root,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|_| internal("release_git_bundle_invalid"))
}

#[cfg(not(target_os = "linux"))]
fn install_noreplace(_: &Path, _: &Path) -> Result<(), OrchestratorError> {
    Err(internal("release_git_bundle_invalid"))
}

fn verify_package(bytes: &[u8], expected: &str) -> Result<(), OrchestratorError> {
    if bytes.len() > 8 * 1024 * 1024 || crate::generator::sha256_hex(bytes) != expected {
        return Err(internal("release_git_bundle_invalid"));
    }
    Ok(())
}

fn ready(root: &Path) -> Result<VerifiedGit, OrchestratorError> {
    let invalid = || internal("release_git_bundle_invalid");
    let git = verify_bundle(root).map_err(|_| invalid())?;
    let version = Command::new(git.executable())
        .arg("--version")
        .env_clear()
        .output()
        .map_err(|_| invalid())?;
    if !version.status.success() || version.stdout != b"git version 2.43.0\n" {
        return Err(invalid());
    }
    let helper = Command::new(root.join("git-remote-http"))
        .env_clear()
        .output()
        .map_err(|_| invalid())?;
    if helper.status.code().is_none_or(|code| code == 127) {
        return Err(invalid());
    }
    Ok(git)
}

#[cfg(test)]
mod tests {
    use super::verify_package;
    #[test]
    fn rejects_changed_or_oversized_package_before_extraction() {
        let bytes = b"fixture-package";
        let sha = crate::generator::sha256_hex(bytes);
        assert!(verify_package(bytes, &sha).is_ok());
        assert!(verify_package(b"fixture-tampered", &sha).is_err());
        let large = vec![0; 8 * 1024 * 1024 + 1];
        assert!(verify_package(&large, &crate::generator::sha256_hex(&large)).is_err());
    }
}
