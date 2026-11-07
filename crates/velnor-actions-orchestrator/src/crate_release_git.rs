// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use velnor_actions_mise::release_git_pin::{ReleaseGitExecutable, VerifiedGit};

#[derive(Debug, Clone)]
pub struct ReleaseGit(GitKind);

#[derive(Debug, Clone)]
enum GitKind {
    Verified(VerifiedGit),
    System,
    #[cfg(test)]
    HostFixture,
}

impl ReleaseGit {
    #[must_use]
    pub fn new(git: VerifiedGit) -> Self {
        Self(GitKind::Verified(git))
    }
    #[must_use]
    pub fn verified(&self) -> Option<&VerifiedGit> {
        match &self.0 {
            GitKind::Verified(git) => Some(git),
            GitKind::System => None,
            #[cfg(test)]
            GitKind::HostFixture => None,
        }
    }
    pub(crate) fn system() -> Result<Self, &'static str> {
        check_system_git()?;
        Ok(Self(GitKind::System))
    }
    #[cfg(test)]
    pub(crate) fn host_fixture() -> Self {
        Self(GitKind::HostFixture)
    }
}

impl ReleaseGitExecutable for ReleaseGit {
    fn executable(&self) -> &Path {
        match &self.0 {
            GitKind::Verified(git) => git.executable(),
            GitKind::System => Path::new("/usr/bin/git"),
            #[cfg(test)]
            GitKind::HostFixture => Path::new("/usr/bin/git"),
        }
    }
    fn exec_path(&self) -> &Path {
        match &self.0 {
            GitKind::Verified(git) => git.exec_path(),
            GitKind::System => Path::new("/usr/bin"),
            #[cfg(test)]
            GitKind::HostFixture => Path::new("/usr/bin"),
        }
    }
    fn revalidate(&self) -> Result<(), &'static str> {
        match &self.0 {
            GitKind::Verified(git) => git.revalidate(),
            GitKind::System => check_system_git(),
            #[cfg(test)]
            GitKind::HostFixture => Ok(()),
        }
    }
}

#[cfg(unix)]
fn check_system_git() -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt as _;
    for (path, directory) in [("/usr", true), ("/usr/bin", true), ("/usr/bin/git", false)] {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| "system_git_unavailable")?;
        if metadata.file_type().is_symlink()
            || metadata.uid() != 0
            || metadata.mode() & 0o022 != 0
            || (directory && !metadata.is_dir())
            || (!directory && (!metadata.is_file() || metadata.mode() & 0o111 == 0))
        {
            return Err("system_git_unavailable");
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_system_git() -> Result<(), &'static str> {
    Err("system_git_unavailable")
}
