// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

type Objects = BTreeMap<String, BTreeSet<PathBuf>>;

#[derive(Debug)]
pub(super) struct GitWitness {
    directory: Option<PathBuf>,
    before: Option<Objects>,
}

impl GitWitness {
    pub(super) fn start(argv: &[String]) -> Self {
        if super::archive_witness::cannot_download(argv) {
            return Self {
                directory: None,
                before: Some(BTreeMap::new()),
            };
        }
        let directory = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")))
            .and_then(|home| std::env::current_dir().ok().map(|cwd| cwd.join(home)))
            .map(|home| home.join("git/db"));
        let before = directory.as_deref().and_then(|path| scan(path).ok());
        Self { directory, before }
    }

    pub(super) fn finish(self) -> Option<Vec<String>> {
        let before = self.before?;
        let Some(directory) = self.directory else {
            return Some(Vec::new());
        };
        let after = scan(&directory).ok()?;
        Some(
            after
                .into_iter()
                .filter_map(|(repository, objects)| {
                    objects
                        .iter()
                        .any(|object| {
                            before
                                .get(&repository)
                                .is_none_or(|saved| !saved.contains(object))
                        })
                        .then(|| format!("git/db/{repository}"))
                })
                .collect(),
        )
    }
}

fn entries(path: &Path, budget: &mut usize) -> std::io::Result<Vec<fs::DirEntry>> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(std::io::Error::other("Git database directory is a symlink"));
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut collected = Vec::new();
    for entry in entries {
        *budget = budget
            .checked_sub(1)
            .ok_or_else(|| std::io::Error::other("Git object observation limit exceeded"))?;
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            return Err(std::io::Error::other(
                "Git object observation contains a symlink",
            ));
        }
        collected.push(entry);
    }
    Ok(collected)
}

fn scan(path: &Path) -> std::io::Result<Objects> {
    let mut budget = 32_768;
    let mut ref_bytes = 1_048_576;
    let mut repositories = BTreeMap::new();
    for repository in entries(path, &mut budget)? {
        if !repository.file_type()?.is_dir() {
            return Err(std::io::Error::other("Git database is not a directory"));
        }
        let name = repository
            .file_name()
            .into_string()
            .map_err(|_| std::io::Error::other("Git database name is not UTF-8"))?;
        let mut objects = BTreeSet::new();
        for reference in ["refs", "FETCH_HEAD", "packed-refs"] {
            references(
                &repository.path().join(reference),
                &mut objects,
                &mut budget,
                &mut ref_bytes,
                0,
            )?;
        }
        let database = repository.path().join("objects");
        for directory in entries(&database, &mut budget)? {
            let prefix = directory.file_name();
            let prefix = prefix.to_string_lossy();
            if prefix != "pack" && !(prefix.len() == 2 && hex(&prefix)) {
                continue;
            }
            for object in entries(&directory.path(), &mut budget)? {
                let filename = object.file_name();
                let filename = filename.to_string_lossy();
                let identity = if prefix == "pack" {
                    filename
                        .strip_prefix("pack-")
                        .and_then(|name| name.strip_suffix(".pack"))
                } else {
                    Some(filename.as_ref())
                };
                if identity.is_some_and(|identity| {
                    hex(identity)
                        && if prefix == "pack" {
                            matches!(identity.len(), 40 | 64)
                        } else {
                            matches!(identity.len(), 38 | 62)
                        }
                }) && object.file_type()?.is_file()
                {
                    objects.insert(
                        object
                            .path()
                            .strip_prefix(&database)
                            .map_err(std::io::Error::other)?
                            .to_path_buf(),
                    );
                }
            }
        }
        repositories.insert(name, objects);
    }
    Ok(repositories)
}

fn references(
    path: &Path,
    state: &mut BTreeSet<PathBuf>,
    budget: &mut usize,
    bytes: &mut usize,
    depth: usize,
) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if depth > 8 || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "Git reference observation is unsupported",
        ));
    }
    if metadata.is_dir() {
        for entry in entries(path, budget)? {
            references(&entry.path(), state, budget, bytes, depth + 1)?;
        }
    } else if metadata.is_file() {
        use std::io::Read;
        let mut content = Vec::new();
        fs::File::open(path)?
            .take(u64::try_from(*bytes).unwrap_or(0) + 1)
            .read_to_end(&mut content)?;
        *bytes = bytes.checked_sub(content.len()).ok_or_else(|| {
            std::io::Error::other("Git reference observation exceeds its byte budget")
        })?;
        state.insert(PathBuf::from(format!(
            "{}:{}",
            path.display(),
            serde_json::to_string(&content).map_err(std::io::Error::other)?
        )));
    } else {
        return Err(std::io::Error::other(
            "Git reference observation is unsupported",
        ));
    }
    Ok(())
}

fn hex(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
