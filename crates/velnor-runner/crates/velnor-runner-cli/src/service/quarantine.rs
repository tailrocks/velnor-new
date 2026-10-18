use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::staging::PrivateDirectory;

pub(super) fn uninstall_after_stop(
    stopped: ExitCode,
    path: &Path,
    preflight_file: &std::fs::File,
    expected: &[u8],
) -> ExitCode {
    if stopped != ExitCode::SUCCESS {
        return ExitCode::from(1);
    }
    match quarantine_remove(path, preflight_file, expected) {
        Ok(QuarantineOutcome::Removed | QuarantineOutcome::Missing) if path_is_absent(path) => {
            ExitCode::SUCCESS
        }
        Ok(QuarantineOutcome::Removed | QuarantineOutcome::Missing) => ExitCode::from(1),
        Ok(QuarantineOutcome::Restored | QuarantineOutcome::Recovery(_)) | Err(_) => {
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
pub(super) fn uninstall_after_stop_with(
    stopped: ExitCode,
    path: &Path,
    expected: &[u8],
    before_quarantine: impl FnOnce(),
    after_quarantine: impl FnOnce(),
) -> ExitCode {
    if stopped != ExitCode::SUCCESS {
        return ExitCode::from(1);
    }
    let owned = match open_expected_file(path, expected) {
        Ok(Some(file)) => file,
        Ok(None) if path_is_absent(path) => return ExitCode::SUCCESS,
        Ok(None) | Err(_) => return ExitCode::from(1),
    };
    before_quarantine();
    match quarantine_remove_with(path, &owned, expected, after_quarantine) {
        Ok(QuarantineOutcome::Removed | QuarantineOutcome::Missing) if path_is_absent(path) => {
            ExitCode::SUCCESS
        }
        Ok(QuarantineOutcome::Removed | QuarantineOutcome::Missing) => ExitCode::from(1),
        Ok(QuarantineOutcome::Restored | QuarantineOutcome::Recovery(_)) | Err(_) => {
            ExitCode::from(1)
        }
    }
}

pub(super) fn path_is_absent(path: &Path) -> bool {
    matches!(
        std::fs::symlink_metadata(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    )
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum QuarantineOutcome {
    Removed,
    Missing,
    Restored,
    Recovery(PathBuf),
}

pub(super) fn quarantine_remove(
    path: &Path,
    expected_file: &std::fs::File,
    contents: &[u8],
) -> std::io::Result<QuarantineOutcome> {
    quarantine_remove_with(path, expected_file, contents, || {})
}

pub(super) fn quarantine_remove_with(
    path: &Path,
    expected_file: &std::fs::File,
    contents: &[u8],
    after_quarantine: impl FnOnce(),
) -> std::io::Result<QuarantineOutcome> {
    let (parent, name) = parent_and_name(path)?;
    let parent_directory = open_directory(parent)?;
    quarantine_remove_at(
        &parent_directory,
        parent,
        name,
        expected_file,
        contents,
        after_quarantine,
    )
}

pub(super) fn quarantine_remove_at(
    parent_directory: &std::fs::File,
    parent: &Path,
    name: &std::ffi::OsStr,
    expected_file: &std::fs::File,
    contents: &[u8],
    after_quarantine: impl FnOnce(),
) -> std::io::Result<QuarantineOutcome> {
    let quarantine =
        PrivateDirectory::create_at(parent, parent_directory, "velnor-plist-quarantine")?;
    match rustix::fs::renameat_with(
        parent_directory,
        name,
        &quarantine.directory,
        "entry",
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        Ok(()) => {}
        Err(error) if error == rustix::io::Errno::NOENT => {
            quarantine.cleanup()?;
            return Ok(QuarantineOutcome::Missing);
        }
        Err(error) => return Err(error.into()),
    }
    after_quarantine();

    if entry_matches_file(
        &quarantine.directory,
        std::ffi::OsStr::new("entry"),
        expected_file,
        contents,
    )? {
        rustix::fs::unlinkat(&quarantine.directory, "entry", rustix::fs::AtFlags::empty())?;
        quarantine.cleanup()?;
        return Ok(QuarantineOutcome::Removed);
    }

    if let Ok(()) = rustix::fs::renameat_with(
        &quarantine.directory,
        "entry",
        parent_directory,
        name,
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        quarantine.cleanup()?;
        Ok(QuarantineOutcome::Restored)
    } else {
        quarantine.preserve_for_recovery();
        let recovery = quarantine.path.join("entry");
        eprintln!(
            "Velnor preserved an unverified service file at {}",
            recovery.display()
        );
        Ok(QuarantineOutcome::Recovery(recovery))
    }
}

pub(super) fn existing_matches(path: &Path, contents: &[u8]) -> std::io::Result<bool> {
    Ok(open_expected_file(path, contents)?.is_some())
}

pub(super) fn open_expected_file(
    path: &Path,
    contents: &[u8],
) -> std::io::Result<Option<std::fs::File>> {
    let path_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    let file = open_file(path)?;
    if !same_file(&path_metadata, &file.metadata()?) || !file_matches(&file, contents)? {
        return Ok(None);
    }
    let current_path = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !current_path.is_file()
        || current_path.file_type().is_symlink()
        || !same_file(&current_path, &file.metadata()?)
        || !file_matches(&file, contents)?
    {
        return Ok(None);
    }
    Ok(Some(file))
}

pub(super) fn path_matches_file(
    path: &Path,
    expected_file: &std::fs::File,
    contents: &[u8],
) -> std::io::Result<bool> {
    match open_expected_file(path, contents)? {
        Some(file) => Ok(same_file(&file.metadata()?, &expected_file.metadata()?)),
        None => Ok(false),
    }
}

pub(super) fn entry_matches_file(
    directory: &std::fs::File,
    name: &std::ffi::OsStr,
    expected_file: &std::fs::File,
    contents: &[u8],
) -> std::io::Result<bool> {
    let Ok(entry) = openat_file(directory, name) else {
        return Ok(false);
    };
    if !same_file(&entry.metadata()?, &expected_file.metadata()?)
        || !file_matches(&entry, contents)?
    {
        return Ok(false);
    }
    let Ok(current) = openat_file(directory, name) else {
        return Ok(false);
    };
    Ok(same_file(&current.metadata()?, &entry.metadata()?) && file_matches(&current, contents)?)
}

pub(super) fn file_matches(file: &std::fs::File, contents: &[u8]) -> std::io::Result<bool> {
    let metadata = file.metadata()?;
    let expected_size = u64::try_from(contents.len())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    if !safe_plist_metadata(&metadata) || metadata.len() != expected_size {
        return Ok(false);
    }
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let mut actual = Vec::with_capacity(contents.len());
    Read::by_ref(&mut reader)
        .take(expected_size.saturating_add(1))
        .read_to_end(&mut actual)?;
    Ok(actual == contents)
}

pub(super) fn open_directory(path: &Path) -> std::io::Result<std::fs::File> {
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    Ok(std::fs::File::from(descriptor))
}

pub(super) fn open_file(path: &Path) -> std::io::Result<std::fs::File> {
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    Ok(std::fs::File::from(descriptor))
}

pub(super) fn openat_file(
    directory: &std::fs::File,
    name: &std::ffi::OsStr,
) -> std::io::Result<std::fs::File> {
    let descriptor = rustix::fs::openat(
        directory,
        name,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    Ok(std::fs::File::from(descriptor))
}

pub(super) fn parent_and_name(path: &Path) -> std::io::Result<(&Path, &std::ffi::OsStr)> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    Ok((parent, name))
}

pub(super) fn safe_directory_metadata(metadata: &std::fs::Metadata) -> bool {
    metadata.is_dir()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.mode() & 0o7777 == 0o700
}

pub(super) fn same_directory_path(path: &Path, directory: &std::fs::File) -> std::io::Result<bool> {
    let path_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let directory_metadata = directory.metadata()?;
    Ok(!path_metadata.file_type().is_symlink()
        && safe_directory_metadata(&directory_metadata)
        && same_file(&path_metadata, &directory_metadata))
}

pub(super) fn safe_plist_metadata(metadata: &std::fs::Metadata) -> bool {
    metadata.is_file()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.mode() & 0o7777 == 0o600
}

pub(super) fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}
