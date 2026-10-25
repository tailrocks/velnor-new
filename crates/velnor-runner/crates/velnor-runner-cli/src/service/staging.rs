use std::cell::Cell;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::quarantine::{
    QuarantineOutcome, file_matches, open_directory, openat_file, quarantine_remove_at,
    safe_directory_metadata, same_directory_path, same_file,
};

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

pub(super) struct PrivateDirectory {
    pub(super) path: PathBuf,
    pub(super) directory: std::fs::File,
    pub(super) preserve: Cell<bool>,
    #[cfg(test)]
    pub(super) test_root: bool,
}

impl PrivateDirectory {
    pub(super) fn create(parent: &Path, label: &str) -> std::io::Result<Self> {
        for _ in 0..16 {
            let path = parent.join(format!(
                ".{label}-{}-{}",
                std::process::id(),
                random_suffix()?
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    let directory = match open_directory(&path) {
                        Ok(directory) => directory,
                        Err(error) => {
                            eprintln!(
                                "Velnor preserved an unverified temporary directory at {}",
                                path.display()
                            );
                            return Err(error);
                        }
                    };
                    if !safe_directory_metadata(&directory.metadata()?)
                        || !same_directory_path(&path, &directory)?
                    {
                        eprintln!(
                            "Velnor preserved an unverified temporary directory at {}",
                            path.display()
                        );
                        return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
                    }
                    return Ok(Self {
                        path,
                        directory,
                        preserve: Cell::new(false),
                        #[cfg(test)]
                        test_root: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }

    pub(super) fn create_at(
        parent: &Path,
        parent_directory: &std::fs::File,
        label: &str,
    ) -> std::io::Result<Self> {
        for _ in 0..16 {
            let name = std::ffi::OsString::from(format!(
                ".{label}-{}-{}",
                std::process::id(),
                random_suffix()?
            ));
            let mode = rustix::fs::Mode::from_bits_truncate(0o700);
            match rustix::fs::mkdirat(parent_directory, &name, mode) {
                Ok(()) => {}
                Err(error) if error == rustix::io::Errno::EXIST => continue,
                Err(error) => return Err(error.into()),
            }
            let descriptor = match rustix::fs::openat(
                parent_directory,
                &name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            ) {
                Ok(descriptor) => descriptor,
                Err(error) => {
                    eprintln!(
                        "Velnor preserved an unverified temporary directory at {}",
                        parent.join(&name).display()
                    );
                    return Err(error.into());
                }
            };
            let directory = std::fs::File::from(descriptor);
            let entry = match rustix::fs::openat(
                parent_directory,
                &name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            ) {
                Ok(entry) => std::fs::File::from(entry),
                Err(error) => {
                    eprintln!(
                        "Velnor preserved an unverified temporary directory at {}",
                        parent.join(&name).display()
                    );
                    return Err(error.into());
                }
            };
            if !safe_directory_metadata(&directory.metadata()?)
                || !same_file(&directory.metadata()?, &entry.metadata()?)
            {
                eprintln!(
                    "Velnor preserved an unverified temporary directory at {}",
                    parent.join(&name).display()
                );
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            return Ok(Self {
                path: parent.join(name),
                directory,
                preserve: Cell::new(false),
                #[cfg(test)]
                test_root: false,
            });
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }

    #[cfg(test)]
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub(super) fn create_test_root(parent: &Path, label: &str) -> std::io::Result<Self> {
        let mut root = Self::create(parent, label)?;
        root.test_root = true;
        Ok(root)
    }

    pub(super) fn create_payload(&self, contents: &[u8]) -> std::io::Result<std::fs::File> {
        if !self.path_matches_directory()? {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        let descriptor = rustix::fs::openat(
            &self.directory,
            "payload",
            rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_bits_truncate(0o600),
        )?;
        let mut file = std::fs::File::from(descriptor);
        file.write_all(contents)?;
        file.sync_all()?;
        rustix::fs::fchmod(&file, rustix::fs::Mode::from_bits_truncate(0o600))?;
        file.sync_all()?;
        if !self.payload_matches(&file, contents)? {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
        Ok(file)
    }

    pub(super) fn path_matches_directory(&self) -> std::io::Result<bool> {
        same_directory_path(&self.path, &self.directory)
    }

    pub(super) fn payload_matches(
        &self,
        payload: &std::fs::File,
        contents: &[u8],
    ) -> std::io::Result<bool> {
        if !file_matches(payload, contents)? {
            return Ok(false);
        }
        let entry = match openat_file(&self.directory, std::ffi::OsStr::new("payload")) {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(same_file(&payload.metadata()?, &entry.metadata()?) && file_matches(&entry, contents)?)
    }

    pub(super) fn remove_payload_if_owned(
        &self,
        payload: &std::fs::File,
        contents: &[u8],
    ) -> std::io::Result<()> {
        self.remove_payload_if_owned_with(payload, contents, || {})
    }

    pub(super) fn remove_payload_if_owned_with(
        &self,
        payload: &std::fs::File,
        contents: &[u8],
        before_quarantine: impl FnOnce(),
    ) -> std::io::Result<()> {
        self.remove_payload_if_owned_inner(payload, contents, before_quarantine)
    }

    fn remove_payload_if_owned_inner(
        &self,
        payload: &std::fs::File,
        contents: &[u8],
        before_quarantine: impl FnOnce(),
    ) -> std::io::Result<()> {
        if !self.path_matches_directory()? || !self.payload_matches(payload, contents)? {
            self.preserve_for_recovery();
            eprintln!(
                "Velnor preserved an unverified staging directory at {}",
                self.path.join("payload").display()
            );
            return Ok(());
        }
        before_quarantine();
        match quarantine_remove_at(
            &self.directory,
            &self.path,
            std::ffi::OsStr::new("payload"),
            payload,
            contents,
            || {},
        )? {
            QuarantineOutcome::Removed | QuarantineOutcome::Missing
                if self.path_matches_directory()? => {}
            QuarantineOutcome::Restored | QuarantineOutcome::Recovery(_) => {
                self.preserve_for_recovery();
                eprintln!(
                    "Velnor preserved an unverified staged file at {}",
                    self.path.join("payload").display()
                );
            }
            QuarantineOutcome::Removed | QuarantineOutcome::Missing => {
                self.preserve_for_recovery();
                eprintln!(
                    "Velnor preserved a changed staging directory at {}",
                    self.path.display()
                );
            }
        }
        Ok(())
    }

    pub(super) fn preserve_for_recovery(&self) {
        self.preserve.set(true);
    }

    pub(super) fn cleanup(&self) -> std::io::Result<()> {
        if self.preserve.get() {
            return Ok(());
        }
        if !self.path_matches_directory()? {
            eprintln!(
                "Velnor left a changed staging directory untouched at {}",
                self.path.display()
            );
            return Ok(());
        }
        match std::fs::remove_dir(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                self.preserve.set(true);
                eprintln!(
                    "Velnor preserved a temporary recovery directory at {}",
                    self.path.display()
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

fn random_suffix() -> std::io::Result<String> {
    let mut random = [0; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let mut hex = [0; 32];
    for (index, byte) in random.into_iter().enumerate() {
        hex[index * 2] = HEX_DIGITS[usize::from(byte >> 4)];
        hex[index * 2 + 1] = HEX_DIGITS[usize::from(byte & 0x0f)];
    }
    String::from_utf8(hex.to_vec())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        #[cfg(test)]
        if self.test_root {
            if self.path_matches_directory().unwrap_or(false) {
                drop(std::fs::remove_dir_all(&self.path));
            }
            return;
        }
        drop(self.cleanup());
    }
}
