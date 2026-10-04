use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::index::repository::RepositoryContext;
use super::super::index::repository_format::IndexObjectFormat;
pub(super) use super::super::index::repository_format::config::DirectoryBinding;
use super::super::index::repository_format::config::{self, ConfigSnapshot};
use super::super::private_root::{OwnedDirectory, OwnedFile, PrivateGitRoot};
use crate::MiseError;

pub(super) const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub(super) struct Prepared {
    pub(super) binding: DirectoryBinding,
    pub(super) cwd: Option<PathBuf>,
    pub(super) worktree: Option<PathBuf>,
    pub(super) objects: Option<PathBuf>,
    pub(super) index: Option<PathBuf>,
    pub(super) bindings: SourceBindings,
}

#[derive(Debug)]
pub(super) struct SourceBindings {
    pub(super) dirs: Vec<(PathBuf, DirectoryBinding, &'static str)>,
    pub(super) files: Vec<ConfigSnapshot>,
    pub(super) forbidden: Vec<PathBuf>,
    pub(super) pack_dirs: Vec<PathBuf>,
}

impl SourceBindings {
    pub(super) fn empty(cwd: PathBuf, binding: DirectoryBinding) -> Self {
        Self {
            dirs: vec![(cwd, binding, "owner_cwd")],
            files: Vec::new(),
            forbidden: Vec::new(),
            pack_dirs: Vec::new(),
        }
    }

    pub(super) fn verify(&self) -> io::Result<()> {
        for (path, binding, label) in &self.dirs {
            config::verify_directory(path, binding, label)?;
        }
        for file in &self.files {
            file.verify()?;
        }
        reject_forbidden(&self.forbidden)?;
        for pack in &self.pack_dirs {
            reject_promisor_packs(pack)?;
        }
        Ok(())
    }
}

pub(super) fn prepare(
    private_root: &PrivateGitRoot,
    source_cwd: &Path,
    source: Option<&RepositoryContext>,
    common: Option<&Path>,
    has_index: bool,
    expected: Option<IndexObjectFormat>,
    bounds: &super::super::Bounds<'_>,
) -> Result<Prepared, MiseError> {
    private_root
        .verify_binding()
        .map_err(|error| spawn_failed(&error))?;
    for directory in [
        OwnedDirectory::Dir,
        OwnedDirectory::Info,
        OwnedDirectory::Refs,
        OwnedDirectory::Heads,
        OwnedDirectory::Objects,
    ] {
        private_root
            .create_directory(directory)
            .map_err(|error| spawn_failed(&error))?;
    }
    let gitdir = private_root.directory_path(OwnedDirectory::Dir);
    let (_, binding) = config::canonical_directory(gitdir, "private_gitdir")
        .map_err(|error| spawn_failed(&error))?;
    match source {
        Some(context) => super::source::prepare_source(
            binding,
            private_root,
            super::source::SourceGitInputs {
                source_cwd,
                context,
                common,
                has_index,
                expected,
            },
            bounds,
        ),
        None => prepare_without_source(
            private_root,
            binding,
            source_cwd,
            common,
            has_index,
            expected,
        ),
    }
}

fn prepare_without_source(
    private_root: &PrivateGitRoot,
    binding: DirectoryBinding,
    source_cwd: &Path,
    common: Option<&Path>,
    has_index: bool,
    expected: Option<IndexObjectFormat>,
) -> Result<Prepared, MiseError> {
    if common.is_some() || has_index || expected.is_some() {
        return Err(invalid("no_index_source_binding"));
    }
    private_root
        .write_file(OwnedFile::Head, b"ref: refs/heads/velnor-private-unborn\n")
        .map_err(|error| spawn_failed(&error))?;
    let (cwd, cwd_binding) = config::canonical_directory(source_cwd, "owner_cwd")
        .map_err(|error| spawn_failed(&error))?;
    Ok(Prepared {
        binding,
        cwd: Some(cwd.clone()),
        worktree: None,
        objects: None,
        index: None,
        bindings: SourceBindings::empty(cwd, cwd_binding),
    })
}

pub(super) fn verify_file(root: &PrivateGitRoot, slot: OwnedFile) -> io::Result<()> {
    root.verify_binding()?;
    let path = root.slot_path(slot);
    let meta = std::fs::symlink_metadata(path)
        .map_err(|error| path_error(&error, "private_file", path))?;
    if !meta.file_type().is_file() || meta.nlink() != 1 || meta.mode() & 0o7777 != 0o600 {
        return Err(io_invalid("private_file_metadata"));
    }
    Ok(())
}

pub(super) fn route(command: &mut Command, gitdir: &Path) {
    command.env("GIT_DIR", gitdir);
    command.env("GIT_COMMON_DIR", gitdir);
}

pub(super) fn verify_directory(
    path: &Path,
    binding: &DirectoryBinding,
    label: &str,
) -> io::Result<()> {
    config::verify_directory(path, binding, label)
}

pub(super) fn reject_forbidden(paths: &[PathBuf]) -> io::Result<()> {
    for path in paths {
        match std::fs::symlink_metadata(path) {
            Ok(_) => return Err(io_invalid("unsupported_source_metadata")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(path_error(&error, "source_metadata", path)),
        }
    }
    Ok(())
}

pub(super) fn reject_promisor_packs(pack: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(pack) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(path_error(&error, "pack_directory", pack)),
        Ok(meta) if !meta.file_type().is_dir() => {
            return Err(io_invalid("pack_not_directory"));
        }
        Ok(_) => {}
    }
    for (count, entry) in std::fs::read_dir(pack)?.enumerate() {
        if count >= 65_536 {
            return Err(io_invalid("pack_directory_too_large"));
        }
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "promisor")
        {
            return Err(io_invalid("unsupported_promisor_pack"));
        }
    }
    Ok(())
}

pub(super) fn spawn_failed(error: &io::Error) -> MiseError {
    MiseError::SpawnFailed {
        program: "git".to_owned(),
        message: error.to_string(),
    }
}

pub(super) fn invalid(code: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "git_owned_context".to_owned(),
        value: code.to_owned(),
    }
}

pub(super) fn io_invalid(code: &'static str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("private_git_owned_context:{code}"),
    )
}

pub(super) fn path_error(error: &io::Error, label: &str, path: &Path) -> io::Error {
    io::Error::new(
        error.kind(),
        format!(
            "private_git_owned_context:{label}:{}:{error}",
            path.display()
        ),
    )
}
