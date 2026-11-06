// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    io::Read,
    os::fd::{AsFd, OwnedFd},
    path::Path,
    time::Instant,
};

use rustix::fs::{self, Mode, OFlags, RenameFlags};
use sha2::{Digest, Sha256};

use crate::{
    OrchestratorError,
    crate_release_custody::{PublisherRoot, PublisherView},
    crate_release_github::GitHubAuthorized,
    crate_release_tool_artifact::StagedToolchain,
    internal::internal,
};

pub(crate) struct ProjectedToolchain {
    staged: StagedToolchain,
    view_device: u64,
    view_inode: u64,
    cargo_sha256: String,
    rustc_sha256: String,
    rustdoc_sha256: String,
}

impl ProjectedToolchain {
    pub(crate) fn project(
        private: &PublisherRoot,
        view: &PublisherView,
        staged: StagedToolchain,
        approved: &GitHubAuthorized,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
    ) -> Result<Self, OrchestratorError> {
        let invalid = || internal("crate_release_tool_projection_invalid");
        private.revalidate().map_err(|_| invalid())?;
        view.revalidate().map_err(|_| invalid())?;
        if std::env::consts::ARCH != "x86_64"
            || staged.authorization() != approved
            || staged.intent() != intent
            || staged.path() != private.path().join("velnor/rustup")
            || Instant::now() >= staged.deadline()
        {
            return Err(invalid());
        }
        verify_installed(private.path(), staged.path(), intent, staged.deadline())?;
        private.revalidate().map_err(|_| invalid())?;
        view.revalidate().map_err(|_| invalid())?;
        let velnor = open_directory(private.descriptor(), "velnor")?;
        fs::renameat_with(
            &velnor,
            "rustup",
            view.descriptor(),
            "rustup",
            RenameFlags::NOREPLACE,
        )
        .map_err(|_| invalid())?;
        let rustup = open_directory(view.descriptor(), "rustup")?;
        fs::fchmod(&rustup, Mode::from_raw_mode(0o711)).map_err(|_| invalid())?;
        let toolchains = open_directory(&rustup, "toolchains")?;
        fs::fchmod(&toolchains, Mode::from_raw_mode(0o711)).map_err(|_| invalid())?;
        verify_installed(
            private.path(),
            &view.path().join("rustup"),
            intent,
            staged.deadline(),
        )?;
        private.revalidate().map_err(|_| invalid())?;
        view.revalidate().map_err(|_| invalid())?;
        let target = format!("{}-x86_64-unknown-linux-gnu", intent.rust_version);
        let tree = open_directory(&toolchains, &target)?;
        let bin = open_directory(&tree, "bin")?;
        let cargo_sha256 = hash_tool(&bin, "cargo", staged.deadline())?;
        let rustc_sha256 = hash_tool(&bin, "rustc", staged.deadline())?;
        let rustdoc_sha256 = hash_tool(&bin, "rustdoc", staged.deadline())?;
        let metadata = fs::fstat(view.descriptor()).map_err(|_| invalid())?;
        if Instant::now() >= staged.deadline() {
            return Err(invalid());
        }
        Ok(Self {
            staged,
            view_device: metadata.st_dev,
            view_inode: metadata.st_ino,
            cargo_sha256,
            rustc_sha256,
            rustdoc_sha256,
        })
    }

    pub(crate) fn revalidate_view(&self, view: &PublisherView) -> Result<(), OrchestratorError> {
        let invalid = || internal("crate_release_tool_projection_invalid");
        view.revalidate().map_err(|_| invalid())?;
        let metadata = fs::fstat(view.descriptor()).map_err(|_| invalid())?;
        if metadata.st_dev != self.view_device
            || metadata.st_ino != self.view_inode
            || Instant::now() >= self.staged.deadline()
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn verify_source_separation(
        &self,
        view: &PublisherView,
    ) -> Result<(), OrchestratorError> {
        let invalid = || internal("crate_release_tool_projection_invalid");
        self.revalidate_view(view)?;
        let source = open_directory(view.descriptor(), "source")?;
        let rustup = open_directory(view.descriptor(), "rustup")?;
        let source_meta = fs::fstat(&source).map_err(|_| invalid())?;
        let rustup_meta = fs::fstat(&rustup).map_err(|_| invalid())?;
        if (source_meta.st_dev, source_meta.st_ino) == (rustup_meta.st_dev, rustup_meta.st_ino) {
            return Err(invalid());
        }
        let toolchains = open_directory(&rustup, "toolchains")?;
        let target = format!(
            "{}-x86_64-unknown-linux-gnu",
            self.staged.intent().rust_version
        );
        let tree = open_directory(&toolchains, &target)?;
        let bin = open_directory(&tree, "bin")?;
        for (name, expected) in [
            ("cargo", &self.cargo_sha256),
            ("rustc", &self.rustc_sha256),
            ("rustdoc", &self.rustdoc_sha256),
        ] {
            if hash_tool(&bin, name, self.staged.deadline())? != *expected {
                return Err(invalid());
            }
        }
        self.revalidate_view(view)?;
        Ok(())
    }

    pub(crate) fn staged(&self) -> &StagedToolchain {
        &self.staged
    }

    pub(crate) fn cargo_sha256(&self) -> &str {
        &self.cargo_sha256
    }
    pub(crate) fn rustc_sha256(&self) -> &str {
        &self.rustc_sha256
    }
    pub(crate) fn rustdoc_sha256(&self) -> &str {
        &self.rustdoc_sha256
    }
}

fn verify_installed(
    private: &Path,
    rustup: &Path,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    deadline: Instant,
) -> Result<(), OrchestratorError> {
    use velnor_actions_workflow_renderer::verifier_builders::{
        ToolchainPhase, verify_rust_owned_argv,
    };
    let invalid = || internal("crate_release_rust_verification_failed");
    let catalog = crate::crate_release_source::source_tool_catalog(&intent.rust_version)
        .map_err(|_| invalid())?;
    let argv = verify_rust_owned_argv(&catalog, ToolchainPhase::Installed);
    if argv.len() != 3 || argv[0] != "sh" || argv[1] != "-c" {
        return Err(invalid());
    }
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(invalid)?;
    let budget = velnor_actions_mise::CheckDeadline::after(remaining).map_err(|_| invalid())?;
    let mut command = std::process::Command::new("/bin/sh");
    command
        .args(&argv[1..])
        .current_dir(private)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", private)
        .env("RUNNER_TEMP", private)
        .env("RUSTUP_HOME", rustup)
        .env("MISE_RUSTUP_HOME", rustup)
        .env("CARGO_HOME", private.join("velnor/cargo"));
    let (output, _drain) = velnor_actions_mise::command::run_prepared_quiescent_until(
        &mut command,
        1024 * 1024,
        budget,
    )
    .map_err(|_| invalid())?;
    if !output.success || Instant::now() >= deadline {
        return Err(invalid());
    }
    Ok(())
}

fn open_directory(parent: &impl AsFd, name: &str) -> Result<OwnedFd, OrchestratorError> {
    let invalid = || internal("crate_release_tool_projection_invalid");
    let directory = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| invalid())?;
    let metadata = fs::fstat(&directory).map_err(|_| invalid())?;
    if metadata.st_uid != 0 || metadata.st_mode & 0o022 != 0 {
        return Err(invalid());
    }
    Ok(directory)
}

fn hash_tool(bin: &impl AsFd, name: &str, deadline: Instant) -> Result<String, OrchestratorError> {
    let invalid = || internal("crate_release_tool_projection_invalid");
    let fd = fs::openat(
        bin,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| invalid())?;
    let metadata = fs::fstat(&fd).map_err(|_| invalid())?;
    if metadata.st_uid != 0
        || metadata.st_mode & 0o170000 != 0o100000
        || metadata.st_mode & 0o022 != 0
        || metadata.st_mode & 0o111 == 0
        || metadata.st_nlink != 1
        || metadata.st_size <= 0
        || metadata.st_size > 256 * 1024 * 1024
    {
        return Err(invalid());
    }
    let mut file = std::fs::File::from(fd);
    let mut hasher = Sha256::new();
    let mut bytes = [0_u8; 32768];
    loop {
        if Instant::now() >= deadline {
            return Err(invalid());
        }
        let size = file.read(&mut bytes).map_err(|_| invalid())?;
        if size == 0 {
            break;
        }
        hasher.update(&bytes[..size]);
    }
    let mut result = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        write!(&mut result, "{byte:02x}").map_err(|_| invalid())?;
    }
    Ok(result)
}
