//! Fixed private slots are the only filesystem mutation destinations.
use super::{OwnedDirectory, OwnedFile, PrivateGitRoot, open_bound_directory};
use std::path::{Path, PathBuf};
use std::{fs, io};

impl PrivateGitRoot {
    pub(in crate::command::git) fn create_directory(
        &self,
        slot: OwnedDirectory,
    ) -> io::Result<PathBuf> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, MetadataExt};
            self.verify_binding()?;
            let path = self.directory_path(slot);
            self.verify_owned_parent(path)?;
            fs::DirBuilder::new().mode(0o700).create(path)?;
            let handle = open_bound_directory(path)?;
            if handle.metadata()?.mode() & 0o7777 != 0o700 {
                return Err(io::Error::other("private_git_directory:mode_invalid"));
            }
            self.verify_binding()?;
            Ok(path.to_path_buf())
        }
        #[cfg(not(unix))]
        {
            let _ = slot;
            Err(io::Error::other("private_git_root:unsupported_platform"))
        }
    }

    pub(in crate::command::git) fn write_file(
        &self,
        slot: OwnedFile,
        bytes: &[u8],
    ) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::io::Write;
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            #[cfg(target_os = "linux")]
            let flags = 0x20000 | 0x800;
            #[cfg(target_os = "macos")]
            let flags = 0x100 | 0x4;
            self.verify_binding()?;
            let path = self.slot_path(slot);
            self.verify_owned_parent(path)?;
            let mut handle = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .custom_flags(flags)
                .mode(0o600)
                .open(path)?;
            handle.write_all(bytes)?;
            let held = handle.metadata()?;
            let named = fs::symlink_metadata(path)?;
            if !named.file_type().is_file()
                || named.nlink() != 1
                || named.mode() & 0o7777 != 0o600
                || (named.dev(), named.ino(), named.len()) != (held.dev(), held.ino(), held.len())
            {
                return Err(io::Error::other("private_git_file:binding_changed"));
            }
            self.verify_binding()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (slot, bytes);
            Err(io::Error::other("private_git_root:unsupported_platform"))
        }
    }

    pub(in crate::command::git) fn replace_contents(
        &self,
        slot: OwnedFile,
        bytes: &[u8],
    ) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::io::Write;
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            #[cfg(target_os = "linux")]
            let flags = 0x20000 | 0x800;
            #[cfg(target_os = "macos")]
            let flags = 0x100 | 0x4;
            self.verify_binding()?;
            self.verify_owned_file(slot)?;
            let path = self.slot_path(slot);
            self.verify_owned_parent(path)?;
            let before = fs::symlink_metadata(path)?;
            let mut handle = fs::OpenOptions::new()
                .write(true)
                .custom_flags(flags)
                .open(path)?;
            let held = handle.metadata()?;
            if !held.file_type().is_file()
                || held.nlink() != 1
                || held.mode() & 0o7777 != 0o600
                || (held.dev(), held.ino()) != (before.dev(), before.ino())
            {
                return Err(io::Error::other("private_git_file:binding_changed"));
            }
            self.verify_owned_file(slot)?;
            let named = fs::symlink_metadata(path)?;
            if (named.dev(), named.ino()) != (held.dev(), held.ino()) {
                return Err(io::Error::other("private_git_file:binding_changed"));
            }
            handle.set_len(0)?;
            handle.write_all(bytes)?;
            let named = fs::symlink_metadata(path)?;
            if (named.dev(), named.ino(), named.len())
                != (held.dev(), held.ino(), bytes.len() as u64)
            {
                return Err(io::Error::other("private_git_file:binding_changed"));
            }
            self.verify_owned_file(slot)?;
            self.verify_binding()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (slot, bytes);
            Err(io::Error::other("private_git_root:unsupported_platform"))
        }
    }

    pub(in crate::command::git) fn write_or_match(
        &self,
        slot: OwnedFile,
        bytes: &[u8],
    ) -> io::Result<()> {
        match self.write_file(slot, bytes) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.verify_binding()?;
                let path = self.slot_path(slot);
                self.verify_owned_parent(path)?;
                self.verify_owned_file(slot)?;
                if super::super::index::fs::read_checked(
                    path,
                    64 * 1024 * 1024,
                    "private_collision",
                )? != bytes
                {
                    return Err(io::Error::other("private_git_file:collision"));
                }
                self.verify_binding()
            }
            Err(error) => Err(error),
        }
    }

    pub(in crate::command::git) fn install_effective_config(&self) -> io::Result<()> {
        self.verify_binding()?;
        let source = self.slot_path(OwnedFile::EffectiveConfig);
        let target = self.slot_path(OwnedFile::BootstrapConfig);
        self.verify_owned_parent(source)?;
        self.verify_owned_parent(target)?;
        self.verify_owned_file(OwnedFile::EffectiveConfig)?;
        self.verify_owned_file(OwnedFile::BootstrapConfig)?;
        // Native serialization was admitted before this owner-only replacement.
        super::super::index::fs::read_checked(source, 8 * 1024 * 1024, "effective_config")?;
        super::super::index::fs::read_checked(target, 8 * 1024 * 1024, "bootstrap_config")?;
        fs::rename(source, target)?;
        self.verify_binding()
    }

    fn verify_owned_file(&self, slot: OwnedFile) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let path = self.slot_path(slot);
            let named = fs::symlink_metadata(path)?;
            if !named.file_type().is_file() || named.nlink() != 1 || named.mode() & 0o7777 != 0o600
            {
                return Err(io::Error::other("private_git_file:metadata_invalid"));
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = slot;
            Err(io::Error::other("private_git_root:unsupported_platform"))
        }
    }

    fn verify_owned_parent(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("private_git_file:no_parent"))?;
        if !parent.starts_with(&self.path) {
            return Err(io::Error::other("private_git_file:foreign_parent"));
        }
        let mut current = parent;
        while current != self.path {
            let handle = open_bound_directory(current)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if handle.metadata()?.mode() & 0o7777 != 0o700 {
                    return Err(io::Error::other("private_git_directory:mode_invalid"));
                }
            }
            current = current
                .parent()
                .ok_or_else(|| io::Error::other("private_git_file:no_root"))?;
        }
        self.verify_binding()
    }
}
