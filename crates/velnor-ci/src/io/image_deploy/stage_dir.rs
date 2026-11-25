// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use crate::error::CiError;

pub(super) fn no_link_on(to: &Path, within: &Path) -> Result<(), CiError> {
    let below = to.strip_prefix(within).unwrap_or_else(|_| Path::new(""));
    let mut so_far = within.to_path_buf();
    let inspect = |path: &Path| match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(CiError::new(format!(
            "{} is a symbolic link, and the stage directory and its parents must be real directories",
            path.display()
        ))),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CiError::new(format!(
            "cannot inspect {}: {error}",
            path.display()
        ))),
    };
    for part in below.components() {
        so_far.push(part);
        if !inspect(&so_far)? {
            return Ok(());
        }
    }
    inspect(to).map(|_| ())
}

pub(super) fn staged_directory(to: &Path, within: &Path) -> Result<(), CiError> {
    no_link_on(to, within)?;
    match fs::read_dir(to) {
        Ok(entries) => {
            if entries.count() > 0 {
                return Err(CiError::new(format!(
                    "{} is not empty; the stage directory receives only the compiled binaries",
                    to.display()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(to).map_err(|error| {
                CiError::new(format!("cannot create {}: {error}", to.display()))
            })?;
        }
        Err(error) => {
            return Err(CiError::new(format!(
                "cannot read {}: {error}",
                to.display()
            )));
        }
    }
    no_link_on(to, within)
}

pub(super) fn private_directory(under: &Path) -> Result<PathBuf, CiError> {
    let mut random = [0_u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut source| std::io::Read::read_exact(&mut source, &mut random))
        .map_err(|error| CiError::new(format!("cannot read random bytes: {error}")))?;
    let mut name = String::new();
    for byte in random {
        let _ = write!(name, "{byte:02x}");
    }
    let directory = under.join(format!("image-stage-{name}"));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|error| CiError::new(format!("cannot create {}: {error}", directory.display())))?;
    Ok(directory)
}

pub(super) fn only_staged_files(to: &Path, names: &[&str]) -> Result<(), CiError> {
    let entries = fs::read_dir(to)
        .map_err(|error| CiError::new(format!("cannot read {}: {error}", to.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| CiError::new(format!("cannot read {}: {error}", to.display())))?;
        let meta = fs::symlink_metadata(entry.path()).map_err(|error| {
            CiError::new(format!(
                "cannot inspect {}: {error}",
                entry.path().display()
            ))
        })?;
        let name = entry.file_name();
        if !meta.is_file() || !names.iter().any(|known| name == *known) {
            return Err(CiError::new(format!(
                "{} holds {} besides the staged binaries; nothing may be added to the stage directory",
                to.display(),
                name.to_string_lossy()
            )));
        }
    }
    Ok(())
}
