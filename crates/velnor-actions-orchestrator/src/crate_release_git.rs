// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use velnor_actions_mise::release_git_pin::{ReleaseGitExecutable, VerifiedGit};

#[derive(Debug, Clone)]
pub struct ReleaseGit(GitKind);

#[derive(Debug, Clone)]
enum GitKind {
    Verified(VerifiedGit),
    #[cfg(test)]
    HostFixture,
}

impl ReleaseGit {
    pub fn new(git: VerifiedGit) -> Self {
        Self(GitKind::Verified(git))
    }
    pub fn verified(&self) -> Option<&VerifiedGit> {
        match &self.0 {
            GitKind::Verified(git) => Some(git),
            #[cfg(test)]
            GitKind::HostFixture => None,
        }
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
            #[cfg(test)]
            GitKind::HostFixture => Path::new("/usr/bin/git"),
        }
    }
    fn exec_path(&self) -> &Path {
        match &self.0 {
            GitKind::Verified(git) => git.exec_path(),
            #[cfg(test)]
            GitKind::HostFixture => Path::new("/usr/bin"),
        }
    }
    fn revalidate(&self) -> Result<(), &'static str> {
        match &self.0 {
            GitKind::Verified(git) => git.revalidate(),
            #[cfg(test)]
            GitKind::HostFixture => Ok(()),
        }
    }
}
