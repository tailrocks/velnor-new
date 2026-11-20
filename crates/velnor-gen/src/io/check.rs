// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use velnor_model::RepoPath;
use velnor_rust::{
    METADATA_PATH, MetadataFileError, metadata_file, metadata_roots, recorded_metadata,
    reduce_metadata,
};

use super::{
    Error, add_manifests, manifest_file, program_trusting, read_files, read_tracked, toplevel,
};

const MANIFEST: &str = "Cargo.toml";

pub fn check_metadata(directory: &Path) -> Result<(), Error> {
    let root = toplevel(directory)?;
    let inside = fs::canonicalize(directory).map_err(|e| Error::File(directory.into(), e))?;
    let relative = inside
        .strip_prefix(&root)
        .map_err(|_| {
            Error::Repository(format!(
                "{} is outside {}",
                inside.display(),
                root.display()
            ))
        })?
        .join(MANIFEST);
    let manifest =
        RepoPath::new(relative.to_string_lossy()).map_err(|e| Error::Repository(e.to_string()))?;
    let mut view = read_tracked(&root)?;
    add_manifests(&mut view, |paths| read_files(&root, paths))?;
    let mut recorded = read_files(&root, &[METADATA_PATH])?;
    let on_disk = recorded.remove(&METADATA_PATH);
    view.rust_metadata =
        recorded_metadata(&view, on_disk.as_deref()).map_err(Error::MetadataFile)?;
    let output = cargo_metadata(&root, &manifest)?;
    let (workspace, fresh) = reduce_metadata(&root, &output).map_err(Error::Metadata)?;
    view.rust_metadata.insert(workspace, fresh);
    let roots = metadata_roots(&view).map_err(Error::Detect)?;
    view.rust_metadata
        .retain(|workspace, _| roots.contains(workspace));
    let expected = metadata_file(&view).map_err(Error::MetadataFile)?;
    if on_disk.as_deref() == Some(expected.as_bytes()) {
        Ok(())
    } else {
        Err(Error::MetadataFile(MetadataFileError::Differs(manifest)))
    }
}

pub(super) fn cargo_metadata(root: &Path, manifest: &RepoPath) -> Result<Vec<u8>, Error> {
    let path = manifest_file(root, manifest)?;
    let directory = path.parent().unwrap_or(root);
    program_trusting(
        directory,
        root,
        "cargo",
        &[
            OsStr::new("metadata"),
            OsStr::new("--format-version"),
            OsStr::new("1"),
            OsStr::new("--no-deps"),
            OsStr::new("--offline"),
            OsStr::new("--manifest-path"),
            path.as_os_str(),
        ],
    )
}
