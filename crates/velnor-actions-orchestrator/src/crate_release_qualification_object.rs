// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{SourceSnapshot, create_link, invalid, set_executable};
use crate::{
    OrchestratorError,
    crate_release_git::ReleaseGit,
    crate_release_journal::{JournalFile, JournalFileKind},
};
use sha1_checked::{Digest as _, Sha1};
use std::{collections::BTreeSet, io::Write as _, path::Path};
use velnor_actions_mise::GitRequest;

const MAX_OBJECT: usize = 256 * 1024 * 1024;
const MAX_TOTAL: usize = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = crate::release::crate_release_source::MAX_SOURCE_ENTRIES;
const MAX_DEPTH: usize = 128;

pub(super) fn snapshot(
    root: &Path,
    sha: &str,
    git: &ReleaseGit,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<SourceSnapshot, OrchestratorError> {
    let mut total = 0usize;
    let commit = read_object(root, "commit", sha, git, &mut total, deadline)?;
    let first = commit
        .split(|byte| *byte == b'\n')
        .next()
        .ok_or_else(invalid)?;
    let tree_sha = std::str::from_utf8(first.strip_prefix(b"tree ").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    validate_sha(tree_sha)?;
    let directory = tempfile::tempdir().map_err(|_| invalid())?;
    let mut stack = vec![(String::new(), tree_sha.to_owned(), 0usize)];
    let mut material = Material::default();
    let mut entries = 0usize;
    while let Some((prefix, tree, depth)) = stack.pop() {
        deadline.remaining().map_err(|_| invalid())?;
        if depth > MAX_DEPTH {
            return Err(invalid());
        }
        let bytes = read_object(root, "tree", &tree, git, &mut total, deadline)?;
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            let end = rest
                .iter()
                .position(|byte| *byte == 0)
                .ok_or_else(invalid)?;
            if rest.len() < end + 21 {
                return Err(invalid());
            }
            let header = std::str::from_utf8(&rest[..end]).map_err(|_| invalid())?;
            let (mode, name) = header.split_once(' ').ok_or_else(invalid)?;
            if !valid_name(name) {
                return Err(invalid());
            }
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            entries = entries.checked_add(1).ok_or_else(invalid)?;
            if path.len() > 1024 || entries > MAX_ENTRIES {
                return Err(invalid());
            }
            let id = hex(&rest[end + 1..end + 21]);
            rest = &rest[end + 21..];
            if mode == "40000" {
                stack.push((path, id, depth + 1));
                continue;
            }
            if !matches!(mode, "100644" | "100755" | "120000") {
                return Err(invalid());
            }
            let data = read_object(root, "blob", &id, git, &mut total, deadline)?;
            material.add(directory.path(), path, mode, &data)?;
        }
    }
    material.files.sort_by(|a, b| a.path.cmp(&b.path));
    if !crate::crate_release_journal::valid_source_files(&material.files) {
        return Err(invalid());
    }
    deadline.remaining().map_err(|_| invalid())?;
    for (path, target) in &material.links {
        deadline.remaining().map_err(|_| invalid())?;
        std::fs::create_dir_all(path.parent().ok_or_else(invalid)?).map_err(|_| invalid())?;
        create_link(path, target)?;
    }
    deadline.remaining().map_err(|_| invalid())?;
    Ok(SourceSnapshot {
        directory,
        source_sha: sha.to_owned(),
        tree_sha: tree_sha.to_owned(),
        manifests: material.manifests,
        nested_toolchains: material.nested_toolchains,
    })
}

#[derive(Default)]
struct Material {
    manifests: BTreeSet<String>,
    nested_toolchains: Vec<String>,
    files: Vec<JournalFile>,
    links: Vec<(std::path::PathBuf, String)>,
}

impl Material {
    fn add(
        &mut self,
        root: &Path,
        path: String,
        mode: &str,
        data: &[u8],
    ) -> Result<(), OrchestratorError> {
        let destination = root.join(&path);
        if destination
            .file_name()
            .is_some_and(|value| value == "Cargo.toml")
        {
            self.manifests.insert(path.clone());
        }
        if path
            .rsplit_once('/')
            .is_some_and(|(_, value)| matches!(value, "rust-toolchain" | "rust-toolchain.toml"))
        {
            self.nested_toolchains.push(path.clone());
        }
        let kind = if mode == "120000" {
            let target = std::str::from_utf8(data).map_err(|_| invalid())?.to_owned();
            self.links.push((destination, target.clone()));
            JournalFileKind::Symlink { target }
        } else {
            std::fs::create_dir_all(destination.parent().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(|_| invalid())?;
            file.write_all(data).map_err(|_| invalid())?;
            set_executable(&destination, mode == "100755")?;
            JournalFileKind::Regular {
                executable: mode == "100755",
            }
        };
        self.files.push(JournalFile {
            path,
            sha256: crate::planner::generator::sha256_hex(data),
            kind,
        });
        Ok(())
    }
}

fn read_object(
    root: &Path,
    kind: &str,
    id: &str,
    git: &ReleaseGit,
    total: &mut usize,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    validate_sha(id)?;
    let output = GitRequest::object(kind, id)
        .map_err(|_| invalid())?
        .run_release_in_until(root, git, MAX_OBJECT, deadline)
        .map_err(|_| invalid())?;
    if !output.success {
        return Err(invalid());
    }
    *total = total.checked_add(output.stdout.len()).ok_or_else(invalid)?;
    if *total > MAX_TOTAL || object_sha(kind, &output.stdout)? != id {
        return Err(invalid());
    }
    deadline.remaining().map_err(|_| invalid())?;
    Ok(output.stdout)
}

pub(crate) fn object_sha(kind: &str, bytes: &[u8]) -> Result<String, OrchestratorError> {
    let mut digest = Sha1::new();
    digest.update(format!("{kind} {}\0", bytes.len()).as_bytes());
    digest.update(bytes);
    let hash = digest.try_finalize();
    if hash.has_collision() {
        return Err(invalid());
    }
    Ok(hex(hash.hash()))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 15)]));
    }
    text
}

fn validate_sha(value: &str) -> Result<(), OrchestratorError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid());
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.eq_ignore_ascii_case(".git")
        && !value.contains(['/', '\\'])
        && !value.chars().any(char::is_control)
}
