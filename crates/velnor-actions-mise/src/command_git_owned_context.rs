//! Private Git metadata closure used by read-only discovery children.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::private_root::{OwnedDirectory, OwnedFile, PrivateGitRoot};
use crate::MiseError;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "command_git_owned_context_source.rs"]
mod source;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "command_git_owned_context_support.rs"]
mod support;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::index::repository_format::config::{self, ConfigSnapshot};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use support::{DirectoryBinding, Prepared, SourceBindings};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
pub(in crate::command::git) struct OwnedConfigTarget<'a> {
    root: &'a PrivateGitRoot,
    binding: DirectoryBinding,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl OwnedConfigTarget<'_> {
    pub(in crate::command::git) fn path(&self) -> &Path {
        self.root.slot_path(OwnedFile::ConfigFragment)
    }

    pub(in crate::command::git) fn apply(&self, command: &mut Command) -> io::Result<()> {
        self.root.verify_binding()?;
        let gitdir = self.root.directory_path(OwnedDirectory::Dir);
        support::verify_directory(gitdir, &self.binding, "private_gitdir")?;
        support::verify_file(self.root, OwnedFile::ConfigFragment)?;
        support::route(command, gitdir);
        Ok(())
    }

    pub(in crate::command::git) fn reset_fragment(&self) -> io::Result<()> {
        self.root.replace_contents(OwnedFile::ConfigFragment, &[])
    }

    pub(in crate::command::git) fn fragment_bytes(&self) -> io::Result<Vec<u8>> {
        support::verify_file(self.root, OwnedFile::ConfigFragment)?;
        super::index::fs::read_checked(self.path(), 8 * 1024 * 1024, "private_config_fragment")
    }

    pub(in crate::command::git) fn store_flat(&self, bytes: &[u8]) -> io::Result<()> {
        self.root
            .replace_contents(OwnedFile::EffectiveConfig, bytes)
    }

    pub(in crate::command::git) fn effective_path(&self) -> &Path {
        self.root.slot_path(OwnedFile::EffectiveConfig)
    }

    pub(in crate::command::git) fn verify_effective(&self) -> io::Result<()> {
        support::verify_file(self.root, OwnedFile::EffectiveConfig)
    }

    fn install(&self) -> io::Result<()> {
        self.root.verify_binding()?;
        support::verify_directory(
            self.root.directory_path(OwnedDirectory::Dir),
            &self.binding,
            "private_gitdir",
        )?;
        self.verify_effective()?;
        self.root.install_effective_config()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::command::git) struct OwnedNativeContext<'a> {
    target: OwnedConfigTarget<'a>,
    cwd: Option<PathBuf>,
    worktree: Option<PathBuf>,
    objects: Option<PathBuf>,
    index: Option<PathBuf>,
    bindings: SourceBindings,
    installed_config: Option<ConfigSnapshot>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl<'a> OwnedNativeContext<'a> {
    pub(in crate::command::git) fn prepare(
        private_root: &'a PrivateGitRoot,
        source_cwd: &Path,
        source: Option<&super::index::repository::RepositoryContext>,
        common: Option<&Path>,
        has_index: bool,
        expected: Option<super::index::repository_format::IndexObjectFormat>,
        bounds: &super::Bounds<'_>,
    ) -> Result<Self, MiseError> {
        let prepared = support::prepare(
            private_root,
            source_cwd,
            source,
            common,
            has_index,
            expected,
            bounds,
        )?;
        let target = target(private_root, &prepared, expected)?;
        Ok(Self {
            target,
            cwd: prepared.cwd,
            worktree: prepared.worktree,
            objects: prepared.objects,
            index: prepared.index,
            bindings: prepared.bindings,
            installed_config: None,
        })
    }

    pub(in crate::command::git) fn config_target(&self) -> &OwnedConfigTarget<'a> {
        &self.target
    }

    pub(in crate::command::git) fn install_config(&mut self) -> io::Result<()> {
        self.verify_binding()?;
        self.target.install()?;
        self.installed_config = Some(config::snapshot(
            self.target
                .root
                .slot_path(OwnedFile::BootstrapConfig)
                .to_path_buf(),
            8 * 1024 * 1024,
            "private_installed_config",
        )?);
        self.verify_binding()
    }

    pub(in crate::command::git) fn verify_binding(&self) -> io::Result<()> {
        self.target.root.verify_binding()?;
        support::verify_directory(
            self.target.root.directory_path(OwnedDirectory::Dir),
            &self.target.binding,
            "private_gitdir",
        )?;
        self.bindings.verify()?;
        if let Some(config) = &self.installed_config {
            config.verify()?;
        }
        Ok(())
    }

    pub(in crate::command::git) fn apply(&self, command: &mut Command) -> io::Result<()> {
        self.verify_binding()?;
        if self.installed_config.is_none() {
            return Err(support::io_invalid("private_config_not_installed"));
        }
        support::route(
            command,
            self.target.root.directory_path(OwnedDirectory::Dir),
        );
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        match &self.worktree {
            Some(path) => command.env("GIT_WORK_TREE", path),
            None => command.env_remove("GIT_WORK_TREE"),
        };
        match &self.objects {
            Some(path) => command.env("GIT_OBJECT_DIRECTORY", path),
            None => command.env_remove("GIT_OBJECT_DIRECTORY"),
        };
        match &self.index {
            Some(path) => command.env("GIT_INDEX_FILE", path),
            None => command.env_remove("GIT_INDEX_FILE"),
        };
        command.env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn target<'a>(
    root: &'a PrivateGitRoot,
    prepared: &Prepared,
    expected: Option<super::index::repository_format::IndexObjectFormat>,
) -> Result<OwnedConfigTarget<'a>, MiseError> {
    root.write_file(OwnedFile::BootstrapConfig, bootstrap_config(expected))
        .map_err(|error| support::spawn_failed(&error))?;
    root.write_file(OwnedFile::EffectiveConfig, &[])
        .map_err(|error| support::spawn_failed(&error))?;
    root.write_file(OwnedFile::ConfigFragment, &[])
        .map_err(|error| support::spawn_failed(&error))?;
    Ok(OwnedConfigTarget {
        root,
        binding: prepared.binding,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn bootstrap_config(
    expected: Option<super::index::repository_format::IndexObjectFormat>,
) -> &'static [u8] {
    match expected {
        Some(super::index::repository_format::IndexObjectFormat::Sha256) => {
            b"[core]\n\trepositoryformatversion = 1\n[extensions]\n\tobjectFormat = sha256\n"
        }
        Some(super::index::repository_format::IndexObjectFormat::Sha1) => {
            b"[core]\n\trepositoryformatversion = 0\n"
        }
        None => b"",
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[derive(Debug)]
pub(in crate::command::git) struct OwnedConfigTarget<'a> {
    marker: std::marker::PhantomData<&'a PrivateGitRoot>,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(in crate::command::git) struct OwnedNativeContext<'a> {
    marker: std::marker::PhantomData<&'a PrivateGitRoot>,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
static EMPTY_TARGET: OwnedConfigTarget<'static> = OwnedConfigTarget {
    marker: std::marker::PhantomData,
};

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl OwnedNativeContext<'_> {
    pub(in crate::command::git) fn prepare(
        _: &PrivateGitRoot,
        _: &Path,
        _: Option<&super::index::repository::RepositoryContext>,
        _: Option<&Path>,
        _: bool,
        _: Option<super::index::repository_format::IndexObjectFormat>,
        _: &super::Bounds<'_>,
    ) -> Result<Self, MiseError> {
        Err(invalid("unsupported_platform"))
    }

    pub(in crate::command::git) fn config_target(&self) -> &OwnedConfigTarget<'_> {
        &EMPTY_TARGET
    }

    pub(in crate::command::git) fn install_config(&mut self) -> io::Result<()> {
        Err(unsupported())
    }

    pub(in crate::command::git) fn verify_binding(&self) -> io::Result<()> {
        Err(unsupported())
    }

    pub(in crate::command::git) fn apply(&self, _: &mut Command) -> io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl OwnedConfigTarget<'_> {
    pub(in crate::command::git) fn path(&self) -> &Path {
        Path::new("")
    }

    pub(in crate::command::git) fn effective_path(&self) -> &Path {
        Path::new("")
    }

    pub(in crate::command::git) fn reset_fragment(&self) -> io::Result<()> {
        Err(unsupported())
    }
    pub(in crate::command::git) fn fragment_bytes(&self) -> io::Result<Vec<u8>> {
        Err(unsupported())
    }
    pub(in crate::command::git) fn store_flat(&self, _: &[u8]) -> io::Result<()> {
        Err(unsupported())
    }

    pub(in crate::command::git) fn verify_effective(&self) -> io::Result<()> {
        Err(unsupported())
    }

    pub(in crate::command::git) fn apply(&self, _: &mut Command) -> io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "private_git_owned_context:unsupported_platform",
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn invalid(code: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "git_owned_context".to_owned(),
        value: code.to_owned(),
    }
}
