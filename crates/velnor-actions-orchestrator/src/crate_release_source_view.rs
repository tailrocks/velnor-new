// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OrchestratorError,
    crate_release_git::ReleaseGit,
    crate_release_github::{
        GitHubAdmissionRead, authorize_github_candidate, verify_immutable_tag_rules,
    },
    crate_release_tool_artifact::receipt::AuthenticatedSource,
    internal::internal,
    release::crate_release_custody::PublisherView,
    release::crate_release_projected_tools::ProjectedToolchain,
};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use velnor_actions_mise::{CheckDeadline, release_git_pin::ReleaseGitExecutable};

pub(crate) struct SealedSourceView {
    source: AuthenticatedSource,
    view: PublisherView,
    tools: ProjectedToolchain,
    git: ReleaseGit,
    deadline: CheckDeadline,
    source_device: u64,
    source_inode: u64,
}

impl SealedSourceView {
    pub(crate) fn seal<G: GitHubAdmissionRead>(
        source: AuthenticatedSource,
        view: PublisherView,
        tools: ProjectedToolchain,
        git: ReleaseGit,
        github: &G,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        let invalid = || internal("crate_release_source_view_invalid");
        deadline.remaining().map_err(|_| invalid())?;
        view.revalidate().map_err(|_| invalid())?;
        tools.revalidate_view(&view)?;
        if git.verified().is_none()
            || tools.staged().authorization() != source.receipt().authority()
            || tools.staged().intent() != &source.receipt().data().intent
        {
            return Err(invalid());
        }
        git.revalidate().map_err(|_| invalid())?;
        let checkout = view.path().join("source");
        let sha = &source.receipt().authority().candidate().request.source_sha;
        let files = deadline
            .scope(|| {
                crate::release::crate_release_source::journal_source_inventory(&checkout, sha, &git)
            })
            .map_err(|_| invalid())??;
        if files != source.source().files() {
            return Err(invalid());
        }
        freeze_checkout(&checkout, deadline)?;
        let frozen = deadline
            .scope(|| {
                crate::release::crate_release_source::journal_source_inventory(&checkout, sha, &git)
            })
            .map_err(|_| invalid())??;
        if frozen != source.source().files() {
            return Err(invalid());
        }
        let authority = source.receipt().authority();
        let current = authorize_github_candidate(
            &authority.candidate().request,
            authority.candidate(),
            github,
        )?;
        verify_immutable_tag_rules(&source.receipt().data().intent.repository, github)?;
        if &current != authority {
            return Err(invalid());
        }
        git.revalidate().map_err(|_| invalid())?;
        tools.revalidate_view(&view)?;
        let source_metadata = std::fs::symlink_metadata(&checkout).map_err(|_| invalid())?;
        if !source_metadata.is_dir()
            || source_metadata.file_type().is_symlink()
            || source_metadata.uid() != 0
            || source_metadata.mode() & 0o7777 != 0o555
        {
            return Err(invalid());
        }
        deadline.remaining().map_err(|_| invalid())?;
        Ok(Self {
            source,
            view,
            tools,
            git,
            deadline,
            source_device: source_metadata.dev(),
            source_inode: source_metadata.ino(),
        })
    }

    pub(crate) fn revalidate_source(&self) -> Result<(), OrchestratorError> {
        let invalid = || internal("crate_release_source_view_invalid");
        self.tools.revalidate_view(&self.view)?;
        let descriptor = rustix::fs::openat(
            self.view.descriptor(),
            "source",
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| invalid())?;
        let held = rustix::fs::fstat(&descriptor).map_err(|_| invalid())?;
        let metadata =
            std::fs::symlink_metadata(self.view.path().join("source")).map_err(|_| invalid())?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != 0
            || metadata.mode() & 0o7777 != 0o555
            || metadata.dev() != self.source_device
            || metadata.ino() != self.source_inode
            || held.st_uid != 0
            || held.st_mode & 0o7777 != 0o555
            || held.st_dev != self.source_device
            || held.st_ino != self.source_inode
        {
            return Err(invalid());
        }
        self.tools.revalidate_view(&self.view)?;
        self.deadline.remaining().map_err(|_| invalid())?;
        Ok(())
    }
}

fn freeze_checkout(root: &Path, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_source_view_invalid");
    let initial = std::fs::symlink_metadata(root).map_err(|_| invalid())?;
    if !initial.is_dir() || initial.file_type().is_symlink() || initial.uid() != 0 {
        return Err(invalid());
    }
    let mut pending = vec![PathBuf::from(root)];
    let mut directories = Vec::new();
    let mut entries = 0_usize;
    let mut total = 0_u64;
    while let Some(directory) = pending.pop() {
        deadline.remaining().map_err(|_| invalid())?;
        for item in std::fs::read_dir(&directory).map_err(|_| invalid())? {
            deadline.remaining().map_err(|_| invalid())?;
            let path = item.map_err(|_| invalid())?.path();
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| invalid())?;
            entries = entries
                .checked_add(1)
                .filter(|count| *count <= crate::release::crate_release_source::MAX_SOURCE_ENTRIES)
                .ok_or_else(invalid)?;
            if metadata.uid() != 0
                || metadata.dev() != initial.dev()
                || metadata.mode() & 0o7000 != 0
            {
                return Err(invalid());
            }
            if metadata.file_type().is_symlink() {
                if path
                    .strip_prefix(root)
                    .map_err(|_| invalid())?
                    .starts_with(".git")
                {
                    return Err(invalid());
                }
            } else if metadata.is_dir() {
                pending.push(path.clone());
                directories.push(path);
            } else if metadata.is_file() {
                if metadata.nlink() != 1 {
                    return Err(invalid());
                }
                total = total
                    .checked_add(metadata.len())
                    .filter(|size| *size <= 4 * 1024 * 1024 * 1024)
                    .ok_or_else(invalid)?;
                let mode = if metadata.mode() & 0o111 != 0 {
                    0o555
                } else {
                    0o444
                };
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                    .map_err(|_| invalid())?;
            } else {
                return Err(invalid());
            }
        }
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in directories
        .into_iter()
        .chain(std::iter::once(root.to_path_buf()))
    {
        deadline.remaining().map_err(|_| invalid())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o555))
            .map_err(|_| invalid())?;
    }
    deadline.remaining().map_err(|_| invalid())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::freeze_checkout;
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        process::Command,
        time::Duration,
    };
    use velnor_actions_mise::CheckDeadline;

    #[test]
    #[ignore = "requires isolated root and real systemd"]
    fn frozen_checkout_is_readable_but_not_writable_by_dynamic_user()
    -> Result<(), Box<dyn std::error::Error>> {
        let parent = tempfile::Builder::new()
            .prefix("velnor-source-view-test-")
            .tempdir_in("/run")?;
        std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o711))?;
        let source = parent.path().join("source");
        std::fs::create_dir(&source)?;
        let dot_git = source.join(".git");
        std::fs::create_dir(&dot_git)?;
        let file = source.join("Cargo.toml");
        std::fs::write(&file, "fixture")?;
        std::fs::write(dot_git.join("HEAD"), "fixture")?;
        symlink("Cargo.toml", source.join("link"))?;
        for directory in [&source, &dot_git] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o777))?;
        }
        for item in [&file, &dot_git.join("HEAD")] {
            std::fs::set_permissions(item, std::fs::Permissions::from_mode(0o666))?;
        }
        freeze_checkout(&source, CheckDeadline::after(Duration::from_secs(30))?)?;
        let status = Command::new("/usr/bin/systemd-run")
            .args(["--quiet", "--wait", "--pipe", "--property=DynamicUser=yes", "--property=ProtectSystem=no", "--property=CapabilityBoundingSet=", "--property=NoNewPrivileges=yes"])
            .arg(format!("--property=ReadWritePaths={}", source.display()))
            .args(["/bin/sh", "-c", "test -r \"$1/Cargo.toml\" && test -r \"$1/link\" && test ! -w \"$1/Cargo.toml\" && test ! -w \"$1/.git/HEAD\" && test ! -w \"$1\"", "freeze-test"])
            .arg(&source).status()?;
        assert!(status.success(), "dynamic user accessed writable source");
        Ok(())
    }
}

#[path = "crate_release_source_baseline.rs"]
mod baseline;
#[path = "crate_release_source_execution.rs"]
mod execution;
pub(crate) use execution::{
    ClosedCompletion, ClosedSourceSession, SourceSessionId, SupervisedSourceExecution,
    SupervisedUpload,
};
#[path = "crate_release_source_tools.rs"]
mod tools;
#[cfg(test)]
pub(crate) use tools::ProjectedSourceTools;
