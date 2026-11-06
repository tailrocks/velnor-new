// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal, release::crate_release_custody::PublisherRoot};
use std::{
    io::{Read as _, Write as _},
    os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _},
    path::Path,
};
use velnor_actions_mise::CheckDeadline;

pub(crate) fn acquire(
    private: &PublisherRoot,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_pinned_tool_acquisition_invalid");
    private.revalidate().map_err(|_| invalid())?;
    let semver = velnor_actions_mise::catalog::qualified_cargo_semver_checks(&intent.runner)
        .ok_or_else(invalid)?;
    let plz = velnor_actions_mise::catalog::qualified_release_plz(
        &intent.release_plz_version,
        &intent.runner,
    )
    .ok_or_else(invalid)?;
    let semver_url = format!(
        "https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v{}/cargo-semver-checks-{}.tar.gz",
        semver.version, semver.target
    );
    let plz_url = format!(
        "https://github.com/release-plz/release-plz/releases/download/release-plz-v{}/release-plz-{}.tar.gz",
        intent.release_plz_version, plz.target
    );
    acquire_one(
        private.path(),
        "velnor-semver",
        "cargo-semver-checks",
        &semver_url,
        semver.archive_sha256,
        semver.executable_sha256,
        deadline,
    )?;
    acquire_one(
        private.path(),
        "velnor-release-plz",
        "release-plz",
        &plz_url,
        plz.archive_sha256,
        plz.executable_sha256,
        deadline,
    )?;
    private.revalidate().map_err(|_| invalid())?;
    deadline.remaining().map_err(|_| invalid())?;
    Ok(())
}

fn acquire_one(
    root: &Path,
    directory: &str,
    binary: &str,
    url: &str,
    archive_sha: &str,
    executable_sha: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_pinned_tool_acquisition_invalid");
    deadline.remaining().map_err(|_| invalid())?;
    let directory_path = root.join(directory);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory_path)
        .map_err(|_| invalid())?;
    let archive_path = directory_path.join("archive.tar.gz");
    let mut command = std::process::Command::new("/usr/bin/curl");
    command
        .args([
            "-q",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-filesize",
            "104857600",
            "--connect-timeout",
            "15",
            "--max-time",
            "120",
            "--retry",
            "2",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--output",
        ])
        .arg(&archive_path)
        .arg(url)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root);
    let (output, _drain) = velnor_actions_mise::command::run_prepared_quiescent_until(
        &mut command,
        1024 * 1024,
        deadline,
    )
    .map_err(|_| invalid())?;
    if !output.success {
        return Err(invalid());
    }
    let archive = match crate::safe_read::read_repo_bytes_until(
        root,
        &format!("{directory}/archive.tar.gz"),
        100 * 1024 * 1024,
        deadline,
    )? {
        crate::safe_read::RepoBytes::Bytes(bytes) if !bytes.is_empty() => bytes,
        _ => return Err(invalid()),
    };
    if crate::planner::generator::sha256_hex(&archive) != archive_sha {
        return Err(invalid());
    }
    let bytes = extract_single_binary(&archive, binary, deadline)?;
    if crate::planner::generator::sha256_hex(&bytes) != executable_sha {
        return Err(invalid());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory_path.join(binary))
        .map_err(|_| invalid())?;
    for chunk in bytes.chunks(32768) {
        deadline.remaining().map_err(|_| invalid())?;
        file.write_all(chunk).map_err(|_| invalid())?;
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o500))
        .map_err(|_| invalid())?;
    deadline.remaining().map_err(|_| invalid())?;
    Ok(())
}

fn extract_single_binary(
    archive: &[u8],
    expected: &str,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let invalid = || internal("crate_release_pinned_tool_archive_invalid");
    let mut decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(archive));
    let mut bytes = Vec::new();
    decoder
        .by_ref()
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > 128 * 1024 * 1024 || decoder.into_inner().position() != archive.len() as u64 {
        return Err(invalid());
    }
    deadline.remaining().map_err(|_| invalid())?;
    let mut tar = tar::Archive::new(std::io::Cursor::new(&bytes));
    let mut entries = tar.entries().map_err(|_| invalid())?;
    let mut entry = entries.next().ok_or_else(invalid)?.map_err(|_| invalid())?;
    if !entry.header().entry_type().is_file()
        || entry.path().map_err(|_| invalid())?.as_ref() != Path::new(expected)
        || entry.size() == 0
        || entry.size() > 128 * 1024 * 1024
    {
        return Err(invalid());
    }
    let mut executable = Vec::new();
    entry
        .by_ref()
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut executable)
        .map_err(|_| invalid())?;
    if executable.len() as u64 != entry.size()
        || executable.len() < 20
        || &executable[..6] != b"\x7fELF\x02\x01"
        || &executable[18..20] != b"\x3e\x00"
    {
        return Err(invalid());
    }
    drop(entry);
    if entries.next().is_some() {
        return Err(invalid());
    }
    deadline.remaining().map_err(|_| invalid())?;
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::acquire_one;
    use std::time::Duration;
    use velnor_actions_mise::CheckDeadline;

    #[test]
    #[ignore = "requires public release asset network access"]
    fn real_catalogued_release_assets_are_downloaded_and_verified()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let semver = velnor_actions_mise::catalog::qualified_cargo_semver_checks("ubuntu-24.04")
            .ok_or("missing semver pin")?;
        let release = velnor_actions_mise::catalog::qualified_release_plz("0.4.3", "ubuntu-24.04")
            .ok_or("missing release-plz pin")?;
        let deadline = CheckDeadline::after(Duration::from_secs(300))?;
        let semver_url = format!(
            "https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v{}/cargo-semver-checks-{}.tar.gz",
            semver.version, semver.target
        );
        let release_url = format!(
            "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-{}.tar.gz",
            release.target
        );
        acquire_one(
            root.path(),
            "semver",
            "cargo-semver-checks",
            &semver_url,
            semver.archive_sha256,
            semver.executable_sha256,
            deadline,
        )?;
        acquire_one(
            root.path(),
            "release",
            "release-plz",
            &release_url,
            release.archive_sha256,
            release.executable_sha256,
            deadline,
        )?;
        Ok(())
    }
}
