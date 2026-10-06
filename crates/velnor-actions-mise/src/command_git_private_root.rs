//! One private directory owner for index and no-index Git comparisons.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
#[path = "command_git_private_root_files.rs"]
mod files;
use super::index::repository::RepositoryContext;
use super::index::repository_format::CommonRepository;

const DIRECTORY_NAMES: [&str; 5] = [
    "gitdir",
    "gitdir/info",
    "gitdir/refs",
    "gitdir/refs/heads",
    "gitdir/objects",
];
const FILE_NAMES: [&str; 7] = [
    "index",
    "gitdir/HEAD",
    "gitdir/config",
    "gitdir/effective.config",
    "gitdir/config.fragment",
    "gitdir/info/attributes",
    "gitdir/info/exclude",
];

#[cfg(test)]
#[path = "command_git_private_root_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(super) enum OwnedDirectory {
    Dir,
    Info,
    Refs,
    Heads,
    Objects,
}

#[derive(Clone, Copy)]
pub(super) enum OwnedFile {
    Index,
    Head,
    BootstrapConfig,
    EffectiveConfig,
    ConfigFragment,
    InfoAttributes,
    InfoExclude,
}

#[derive(Debug)]
pub(super) struct BoundCwd {
    path: PathBuf,
    #[cfg(unix)]
    identity: (u64, u64, u32, u32),
    handle: fs::File,
}

impl BoundCwd {
    pub(super) fn from_owner(cwd: Option<&PathBuf>) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = std::env::current_dir()?;
            let input = cwd.map_or(current.clone(), |cwd| current.join(cwd));
            if !fs::symlink_metadata(&input)?.file_type().is_dir() {
                return Err(io::Error::other("private_git_cwd:invalid_directory"));
            }
            let path = fs::canonicalize(input)?;
            let handle = open_bound_directory(&path)?;
            let metadata = handle.metadata()?;
            let bound = Self {
                path,
                identity: (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.uid(),
                    metadata.mode(),
                ),
                handle,
            };
            bound.verify_binding()?;
            Ok(bound)
        }
        #[cfg(not(unix))]
        {
            let _ = cwd;
            Err(io::Error::other("private_git_cwd:unsupported_platform"))
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn verify_binding(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::symlink_metadata(&self.path)?;
            let held = self.handle.metadata()?;
            if metadata.file_type().is_dir()
                && (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.uid(),
                    metadata.mode(),
                ) == self.identity
                && (held.dev(), held.ino(), held.uid(), held.mode()) == self.identity
            {
                return Ok(());
            }
            Err(io::Error::other("private_git_cwd:binding_changed"))
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::other("private_git_cwd:unsupported_platform"))
        }
    }
}

#[derive(Debug)]
pub(super) struct PrivateGitRoot {
    path: PathBuf,
    directories: [PathBuf; 5],
    files: [PathBuf; 7],
    armed: bool,
    #[cfg(unix)]
    identity: (u64, u64, u32),
    handle: fs::File,
}

impl PrivateGitRoot {
    pub(super) fn create_repository(
        source: &RepositoryContext,
        common: &CommonRepository,
    ) -> io::Result<Self> {
        source.verify_binding()?;
        common.verify()?;
        let mut excluded = vec![source.cwd(), source.gitdir(), common.path()];
        if let Some(worktree) = source.worktree() {
            excluded.push(worktree);
        }
        Self::create(&excluded)
    }

    pub(super) fn create_no_index(
        cwd: &BoundCwd,
        source: Option<&RepositoryContext>,
        common: Option<&CommonRepository>,
    ) -> io::Result<Self> {
        cwd.verify_binding()?;
        match (source, common) {
            (Some(source), Some(common)) => Self::create_repository(source, common),
            (None, None) => Self::create(&[cwd.path()]),
            _ => Err(io::Error::other("private_git_root:source_context_mismatch")),
        }
    }

    fn create(excluded: &[&Path]) -> io::Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::unix::fs::MetadataExt;
            let path = super::index::fs::create_private_directory(excluded)?;
            let handle = open_bound_directory(&path)?;
            let metadata = handle.metadata()?;
            let directories = DIRECTORY_NAMES.map(|name| path.join(name));
            let files = FILE_NAMES.map(|name| path.join(name));
            let root = Self {
                path,
                directories,
                files,
                armed: true,
                identity: (metadata.dev(), metadata.ino(), metadata.uid()),
                handle,
            };
            root.verify_binding()?;
            Ok(root)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = excluded;
            Err(io::Error::other("private_git_root:unsupported_platform"))
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn directory_path(&self, slot: OwnedDirectory) -> &Path {
        &self.directories[slot as usize]
    }

    pub(super) fn slot_path(&self, slot: OwnedFile) -> &Path {
        &self.files[slot as usize]
    }

    pub(super) fn verify_binding(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::symlink_metadata(&self.path)?;
            let held = self.handle.metadata()?;
            if !metadata.file_type().is_dir()
                || metadata.mode() & 0o7777 != 0o700
                || (metadata.dev(), metadata.ino(), metadata.uid()) != self.identity
                || !held.file_type().is_dir()
                || held.mode() & 0o7777 != 0o700
                || (held.dev(), held.ino(), held.uid()) != self.identity
            {
                return Err(io::Error::other("private_git_root:binding_changed"));
            }
            if self.directories != DIRECTORY_NAMES.map(|name| self.path.join(name))
                || self.files != FILE_NAMES.map(|name| self.path.join(name))
            {
                return Err(io::Error::other("private_git_root:cached_slot_changed"));
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::other("private_git_root:unsupported_platform"))
        }
    }

    pub(super) fn finish(mut self) -> io::Result<()> {
        self.cleanup()
    }

    pub(super) fn retain_unreaped(mut self) {
        self.armed = false;
        eprintln!(
            "private_git_root:unreaped_child_retained:{}",
            self.path.display()
        );
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.verify_binding()?;
        match fs::remove_dir_all(&self.path) {
            Ok(()) => {
                self.armed = false;
                Ok(())
            }
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!("private_git_root:cleanup:{}:{error}", self.path.display()),
            )),
        }
    }
}

fn open_bound_directory(path: &Path) -> io::Result<fs::File> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        #[cfg(target_os = "linux")]
        let flags = 0x20000 | 0x800;
        #[cfg(target_os = "macos")]
        let flags = 0x100 | 0x4;
        let before = fs::symlink_metadata(path)?;
        if !before.file_type().is_dir() {
            return Err(io::Error::other("private_git_directory:invalid_type"));
        }
        let handle = fs::OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(path)?;
        let held = handle.metadata()?;
        let after = fs::symlink_metadata(path)?;
        let stamp = |metadata: &fs::Metadata| {
            (
                metadata.dev(),
                metadata.ino(),
                metadata.uid(),
                metadata.mode(),
            )
        };
        if !held.file_type().is_dir()
            || !after.file_type().is_dir()
            || stamp(&before) != stamp(&held)
            || stamp(&held) != stamp(&after)
        {
            return Err(io::Error::other("private_git_directory:binding_changed"));
        }
        Ok(handle)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        Err(io::Error::other(
            "private_git_directory:unsupported_platform",
        ))
    }
}

impl Drop for PrivateGitRoot {
    fn drop(&mut self) {
        if self.armed
            && let Err(error) = self.cleanup()
        {
            eprintln!("private_git_root:cleanup_failed:{error}");
        }
    }
}
