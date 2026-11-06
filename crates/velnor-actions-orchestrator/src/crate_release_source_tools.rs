// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CheckDeadline, MetadataExt, OrchestratorError, Path, PathBuf, PermissionsExt,
    ReleaseGitExecutable, SealedSourceView, internal,
};

pub(crate) struct ProjectedSourceTools {
    pub(super) sealed: SealedSourceView,
    pub(super) git_sha256: String,
    pub(super) git_http_sha256: String,
    pub(super) semver_sha256: String,
    pub(super) release_plz_sha256: String,
    pub(super) compiler_sha256: String,
    pub(super) checker_original: PathBuf,
}

impl SealedSourceView {
    pub(crate) fn project_source_tools(
        self,
        private: &crate::crate_release_custody::PublisherRoot,
    ) -> Result<ProjectedSourceTools, OrchestratorError> {
        use velnor_actions_mise::release_git_pin::{GIT_HTTP_SHA256, GIT_SHA256};
        let invalid = || internal("crate_release_source_tools_invalid");
        self.deadline.remaining().map_err(|_| invalid())?;
        private.revalidate().map_err(|_| invalid())?;
        self.tools.revalidate_view(&self.view)?;
        let git = self.git.verified().ok_or_else(invalid)?;
        git.revalidate().map_err(|_| invalid())?;
        let checker = velnor_actions_mise::catalog::qualified_cargo_semver_checks(
            &self.source.receipt().data().intent.runner,
        )
        .ok_or_else(invalid)?;
        let checker_bytes = match crate::safe_read::read_repo_bytes_until(
            private.path(),
            "velnor-semver/cargo-semver-checks",
            128 * 1024 * 1024,
            self.deadline,
        )? {
            crate::safe_read::RepoBytes::Bytes(bytes) if !bytes.is_empty() => bytes,
            _ => return Err(invalid()),
        };
        if crate::generator::sha256_hex(&checker_bytes) != checker.executable_sha256 {
            return Err(invalid());
        }
        let release_plz = velnor_actions_mise::catalog::qualified_release_plz(
            &self.source.receipt().data().intent.release_plz_version,
            &self.source.receipt().data().intent.runner,
        )
        .ok_or_else(invalid)?;
        let release_plz_bytes = match crate::safe_read::read_repo_bytes_until(
            private.path(),
            "velnor-release-plz/release-plz",
            128 * 1024 * 1024,
            self.deadline,
        )? {
            crate::safe_read::RepoBytes::Bytes(bytes) if !bytes.is_empty() => bytes,
            _ => return Err(invalid()),
        };
        if crate::generator::sha256_hex(&release_plz_bytes) != release_plz.executable_sha256 {
            return Err(invalid());
        }
        let git_bytes = read_checked_executable(
            git.executable(),
            GIT_SHA256,
            16 * 1024 * 1024,
            self.deadline,
        )?;
        let helper_bytes = read_checked_executable(
            &git.exec_path().join("git-remote-http"),
            GIT_HTTP_SHA256,
            16 * 1024 * 1024,
            self.deadline,
        )?;
        project_executables(
            self.view.path(),
            &checker_bytes,
            &release_plz_bytes,
            &git_bytes,
            &helper_bytes,
            self.deadline,
        )?;
        git.revalidate().map_err(|_| invalid())?;
        let compiler_path = std::fs::canonicalize("/usr/bin/cc").map_err(|_| invalid())?;
        let compiler_sha256 = hash_host_compiler(&compiler_path, self.deadline)?;
        self.tools.revalidate_view(&self.view)?;
        private.revalidate().map_err(|_| invalid())?;
        self.deadline.remaining().map_err(|_| invalid())?;
        Ok(ProjectedSourceTools {
            sealed: self,
            git_sha256: GIT_SHA256.to_owned(),
            git_http_sha256: GIT_HTTP_SHA256.to_owned(),
            semver_sha256: checker.executable_sha256.to_owned(),
            release_plz_sha256: release_plz.executable_sha256.to_owned(),
            compiler_sha256,
            checker_original: private.path().join("velnor-semver/cargo-semver-checks"),
        })
    }
}

fn project_executables(
    view: &Path,
    checker_bytes: &[u8],
    release_plz_bytes: &[u8],
    git_bytes: &[u8],
    helper_bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_source_tools_invalid");
    let semver_dir = view.join("semver");
    let git_dir = view.join("git");
    let plz_dir = view.join("release-plz");
    std::fs::create_dir(&semver_dir).map_err(|_| invalid())?;
    std::fs::create_dir(&git_dir).map_err(|_| invalid())?;
    std::fs::create_dir(&plz_dir).map_err(|_| invalid())?;
    for directory in [&semver_dir, &git_dir, &plz_dir] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))
            .map_err(|_| invalid())?;
    }
    write_projected_executable(
        &semver_dir.join("cargo-semver-checks"),
        checker_bytes,
        deadline,
    )?;
    write_projected_executable(&plz_dir.join("release-plz"), release_plz_bytes, deadline)?;
    write_projected_executable(&git_dir.join("git"), git_bytes, deadline)?;
    write_projected_executable(&git_dir.join("git-remote-http"), helper_bytes, deadline)?;
    std::os::unix::fs::symlink("git-remote-http", git_dir.join("git-remote-https"))
        .map_err(|_| invalid())?;
    velnor_actions_mise::release_git_pin::verify_bundle(&git_dir).map_err(|_| invalid())?;
    Ok(())
}

fn read_checked_executable(
    path: &Path,
    expected: &str,
    limit: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    use std::io::Read as _;
    let invalid = || internal("crate_release_source_tools_invalid");
    let before = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.uid() != 0
        || before.mode() & 0o022 != 0
        || before.mode() & 0o111 == 0
        || before.nlink() != 1
        || before.len() == 0
        || before.len() > limit
    {
        return Err(invalid());
    }
    let mut file = std::fs::File::open(path).map_err(|_| invalid())?;
    let opened = file.metadata().map_err(|_| invalid())?;
    if (before.dev(), before.ino()) != (opened.dev(), opened.ino()) {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 32768];
    loop {
        deadline.remaining().map_err(|_| invalid())?;
        let count = file.read(&mut buffer).map_err(|_| invalid())?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() as u64 > limit {
            return Err(invalid());
        }
    }
    let after = file.metadata().map_err(|_| invalid())?;
    if (
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime(),
        before.ctime(),
    ) != (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.ctime(),
    ) || crate::generator::sha256_hex(&bytes) != expected
    {
        return Err(invalid());
    }
    Ok(bytes)
}

fn write_projected_executable(
    path: &Path,
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    use std::io::Write as _;
    let invalid = || internal("crate_release_source_tools_invalid");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| invalid())?;
    for chunk in bytes.chunks(32768) {
        deadline.remaining().map_err(|_| invalid())?;
        file.write_all(chunk).map_err(|_| invalid())?;
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o555))
        .map_err(|_| invalid())?;
    deadline.remaining().map_err(|_| invalid())?;
    Ok(())
}

fn hash_host_compiler(path: &Path, deadline: CheckDeadline) -> Result<String, OrchestratorError> {
    use std::io::Read as _;
    let invalid = || internal("crate_release_source_tools_invalid");
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || metadata.mode() & 0o111 == 0
        || metadata.nlink() != 1
        || metadata.len() == 0
        || metadata.len() > 128 * 1024 * 1024
    {
        return Err(invalid());
    }
    let mut file = std::fs::File::open(path).map_err(|_| invalid())?;
    let opened = file.metadata().map_err(|_| invalid())?;
    if (metadata.dev(), metadata.ino()) != (opened.dev(), opened.ino()) {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 32768];
    loop {
        deadline.remaining().map_err(|_| invalid())?;
        let count = file.read(&mut buffer).map_err(|_| invalid())?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > 128 * 1024 * 1024 {
            return Err(invalid());
        }
    }
    let after = file.metadata().map_err(|_| invalid())?;
    if (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.ctime(),
    ) != (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.ctime(),
    ) || bytes.len() < 20
        || &bytes[..6] != b"\x7fELF\x02\x01"
        || &bytes[18..20] != b"\x3e\x00"
    {
        return Err(invalid());
    }
    deadline.remaining().map_err(|_| invalid())?;
    Ok(crate::generator::sha256_hex(&bytes))
}
