use std::fs;
use std::fs::File;
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use velnor_actions_mise::CheckDeadline;
use velnor_actions_orchestrator_core::OrchestratorError;

use super::{internal, io_error, unsafe_path, unsafe_path_text};

pub(super) struct SafePath {
    pub(super) path: PathBuf,
    pub(super) key: String,
    pub(super) trailing_separator: bool,
}

pub(super) fn archive_format(url: &str) -> Result<super::ArchiveFormat, OrchestratorError> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let path = Path::new(path);
    let extension = path.extension().and_then(|extension| extension.to_str());
    let is_extension = |expected: &str| {
        extension.is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
    };
    let is_tar = path
        .file_stem()
        .and_then(|stem| Path::new(stem).extension())
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("tar"));
    if is_extension("zip") {
        return Ok(super::ArchiveFormat::Zip);
    }
    if is_extension("tgz") || is_extension("crate") || (is_tar && is_extension("gz")) {
        return Ok(super::ArchiveFormat::TarGzip);
    }
    if is_tar && is_extension("xz") {
        return Ok(super::ArchiveFormat::TarXz);
    }
    Err(internal("tool_archive_format"))
}

pub(super) fn safe_path(raw: &[u8]) -> Result<SafePath, OrchestratorError> {
    let text = std::str::from_utf8(raw).map_err(|_| internal("tool_archive_non_utf8_path"))?;
    let trailing_separator = text.ends_with('/');
    if text.is_empty()
        || text.len() > super::MAX_PATH_BYTES
        || text.contains('\0')
        || text.contains('\\')
        || text.starts_with('/')
        || text.as_bytes().get(1) == Some(&b':')
    {
        return Err(unsafe_path_text(text, "archive_path"));
    }
    let mut path = PathBuf::new();
    let mut key = String::new();
    let mut components = 0_usize;
    let parts = text.split('/').collect::<Vec<_>>();
    for (index, component) in parts.iter().enumerate() {
        if component.is_empty() && trailing_separator && index + 1 == parts.len() {
            continue;
        }
        if component.is_empty() || *component == "." || *component == ".." {
            return Err(unsafe_path_text(text, "archive_path_component"));
        }
        components = components.saturating_add(1);
        if components > super::MAX_PATH_COMPONENTS {
            return Err(internal("tool_archive_path_limit"));
        }
        path.push(*component);
        if !key.is_empty() {
            key.push('/');
        }
        key.push_str(component);
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(unsafe_path_text(text, "archive_path_component"));
    }
    Ok(SafePath {
        path,
        key,
        trailing_separator,
    })
}

pub(super) fn ensure_directory(
    destination: &Path,
    relative: &Path,
) -> Result<(), OrchestratorError> {
    let mut current = destination.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(internal("tool_archive_path_component"));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(unsafe_path(&current, "archive_output_symlink"));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(OrchestratorError::OverwriteRefused {
                    path: current.display().to_string(),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| io_error(&current, error))?;
                let created =
                    fs::symlink_metadata(&current).map_err(|error| io_error(&current, error))?;
                if created.file_type().is_symlink() || !created.is_dir() {
                    return Err(unsafe_path(&current, "archive_output_directory"));
                }
            }
            Err(error) => return Err(io_error(&current, error)),
        }
    }
    Ok(())
}

pub(super) fn write_entry<R: Read>(
    destination: &Path,
    relative: &Path,
    reader: &mut R,
    size: u64,
    mode: u32,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    super::check_deadline(deadline)?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    ensure_directory(destination, parent)?;
    let output = destination.join(relative);
    let mut file = create_output(&output)?;
    let copied =
        io::copy(&mut reader.take(size), &mut file).map_err(|error| io_error(&output, error))?;
    if copied != size {
        return Err(internal("tool_archive_truncated_entry"));
    }
    super::check_deadline(deadline)?;
    file.flush().map_err(|error| io_error(&output, error))?;
    set_mode(&output, mode, false)
}

fn create_output(path: &Path) -> Result<File, OrchestratorError> {
    #[cfg(unix)]
    {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let name = path
            .file_name()
            .ok_or_else(|| internal("tool_archive_output_name"))?;
        let parent_fd = rustix::fs::open(
            parent,
            rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| io_error(parent, error))?;
        let descriptor = rustix::fs::openat(
            &parent_fd,
            name,
            rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .map_err(|error| io_error(path, error))?;
        Ok(File::from(descriptor))
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| io_error(path, error))
    }
}

pub(super) fn set_mode(path: &Path, mode: u32, directory: bool) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(unsafe_path(path, "archive_output_type"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))
            .map_err(|error| io_error(path, error))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

pub(super) fn apply_directory_modes(
    destination: &Path,
    mut directories: Vec<(PathBuf, u32)>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    directories.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (path, mode) in directories {
        super::check_deadline(deadline)?;
        set_mode(&destination.join(path), mode, true)?;
    }
    Ok(())
}

pub(super) fn create_destination(destination: &Path) -> Result<(), OrchestratorError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    reject_parent_links(parent)?;
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(unsafe_path(destination, "destination_symlink"))
        }
        Ok(_) => Err(OrchestratorError::OverwriteRefused {
            path: destination.display().to_string(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(destination).map_err(|error| io_error(destination, error))?;
            match fs::symlink_metadata(destination) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
                Ok(_) => Err(unsafe_path(destination, "destination_not_directory")),
                Err(error) => Err(io_error(destination, error)),
            }
        }
        Err(error) => Err(io_error(destination, error)),
    }
}

fn reject_parent_links(path: &Path) -> Result<(), OrchestratorError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| io_error(path, error))?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return Err(internal("tool_archive_parent_escape")),
            Component::Normal(part) => {
                current.push(part);
                let metadata =
                    fs::symlink_metadata(&current).map_err(|error| io_error(&current, error))?;
                if metadata.file_type().is_symlink() {
                    return Err(unsafe_path(&current, "destination_parent_symlink"));
                }
            }
        }
    }
    Ok(())
}
