// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::super::input_scopes::scoped_files;
use super::git;
use crate::error::CiError;
use crate::inputs::{Tracked, TrackedFile, is_representable, parse_ls_files, with_worktree_blobs};
use crate::scope::Scope;

pub(in crate::io) struct TrackedPaths {
    pub top: PathBuf,
    pub paths: Vec<PathBuf>,
}

pub(in crate::io) fn tracked_paths(root: &Path) -> Result<TrackedPaths, CiError> {
    let top = git(root, &["rev-parse", "--show-toplevel"], None)?;
    let top = PathBuf::from(
        String::from_utf8(top)
            .map_err(|e| CiError::new(format!("git rev-parse printed non-UTF-8: {e}")))?
            .trim_end_matches(['\n', '\r']),
    );
    let listed = git(&top, &["ls-files", "-z", "--recurse-submodules"], None)?;
    let mut paths = Vec::new();
    for path in listed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            paths.push(PathBuf::from(std::ffi::OsString::from_vec(path.to_vec())));
        }
        #[cfg(not(unix))]
        {
            paths.push(PathBuf::from(String::from_utf8(path.to_vec()).map_err(
                |e| CiError::new(format!("git ls-files printed non-UTF-8: {e}")),
            )?));
        }
    }
    Ok(TrackedPaths { top, paths })
}

pub(in crate::io) struct CapturedInputs {
    pub tracked: Tracked,
    pub source_identity: String,
    pub sources: Tracked,
}

pub(in crate::io) fn capture_inputs(root: &Path, strict: bool) -> Result<CapturedInputs, CiError> {
    let top = tracked_paths(root)?.top;
    let sources = top_level_files(&top, &Scope::default(), strict)?;
    let source_identity = super::super::delta_source::identity_of(&sources).unwrap_or_default();
    let (_, tracked) = scoped_files(root, None)?;
    Ok(CapturedInputs {
        tracked,
        source_identity,
        sources,
    })
}

pub(in crate::io) fn source_files(root: &Path) -> Result<Tracked, CiError> {
    let top = tracked_paths(root)?.top;
    top_level_files(&top, &Scope::default(), true)
}

pub(in crate::io) fn top_level_files(
    root: &Path,
    scope: &Scope,
    strict: bool,
) -> Result<Tracked, CiError> {
    let index = parse_ls_files(
        &git(
            root,
            &["ls-files", "-s", "-z", "--recurse-submodules"],
            None,
        )?,
        scope,
    );
    let mut present = Vec::with_capacity(index.files.len());
    let mut absent = Vec::new();
    for file in index.files {
        match fs::symlink_metadata(root.join(&file.path)) {
            Ok(metadata) if metadata.is_file() => present.push(TrackedFile {
                mode: worktree_mode(&metadata, &file.mode),
                ..file
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => absent.push(file.path),
            _ if strict => return Err(CiError::new("tracked source is not a regular file")),
            _ => absent.push(file.path),
        }
    }
    let mut paths = String::new();
    for file in &present {
        let path = root.join(&file.path);
        let path = path
            .to_str()
            .ok_or_else(|| CiError::new(format!("{} is not valid UTF-8", path.display())))?;
        paths.push_str(path);
        paths.push('\n');
    }
    let hashes = git(
        root,
        &["hash-object", "--no-filters", "--stdin-paths"],
        Some(paths.as_bytes()),
    )?;
    let hashes = String::from_utf8(hashes)
        .map_err(|e| CiError::new(format!("git hash-object printed non-UTF-8: {e}")))?;
    let mut links = Vec::with_capacity(index.links.len());
    let mut unverifiable_links = index.unverifiable_links;
    for link in index.links {
        let target = match fs::read_link(root.join(&link.path)) {
            Ok(target) => target,
            Err(error) if strict && error.kind() != ErrorKind::NotFound => {
                return Err(CiError::new("tracked source is not a symlink"));
            }
            Err(_) => {
                absent.push(link.path);
                continue;
            }
        };
        match target.to_str() {
            Some(text) if is_representable(text) => links.push(TrackedFile {
                blob: text.to_owned(),
                ..link
            }),
            _ => unverifiable_links += 1,
        }
    }
    Ok(Tracked {
        files: with_worktree_blobs(present, &hashes)?,
        links,
        absent,
        unsupported: index.unsupported,
        unverifiable_links,
        submodules: index.submodules,
    })
}

#[cfg(unix)]
fn worktree_mode(metadata: &fs::Metadata, _index_mode: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    format!("{:o}", 0o100_000 | (metadata.permissions().mode() & 0o7777))
}

#[cfg(not(unix))]
fn worktree_mode(_metadata: &fs::Metadata, index_mode: &str) -> String {
    index_mode.to_owned()
}
