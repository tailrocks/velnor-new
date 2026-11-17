// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    bytes: u64,
    modified: SystemTime,
}

#[derive(Debug)]
pub(super) struct ArchiveWitness {
    cache: Option<PathBuf>,
    before: Option<BTreeMap<PathBuf, Stamp>>,
}

impl ArchiveWitness {
    pub(super) fn start() -> Self {
        let cache = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")))
            .and_then(|home| std::env::current_dir().ok().map(|cwd| cwd.join(home)))
            .map(|home| home.join("registry/cache"));
        let before = cache.as_deref().and_then(|cache| scan(cache).ok());
        Self { cache, before }
    }

    pub(super) fn finish(self) -> Option<Vec<String>> {
        let before = self.before?;
        let after = scan(self.cache.as_deref()?).ok()?;
        let changed = after
            .into_iter()
            .filter(|(path, stamp)| before.get(path) != Some(stamp));
        changed
            .map(|(path, _)| path.file_name()?.to_str().map(str::to_owned))
            .collect::<Option<BTreeSet<_>>>()
            .map(|names| names.into_iter().collect())
    }
}

fn scan(cache: &Path) -> std::io::Result<BTreeMap<PathBuf, Stamp>> {
    let registries = match fs::read_dir(cache) {
        Ok(registries) => registries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error),
    };
    let mut archives = BTreeMap::new();
    for registry in registries {
        let registry = registry?;
        if !fs::metadata(registry.path())?.is_dir() {
            continue;
        }
        for archive in fs::read_dir(registry.path())? {
            let archive = archive?;
            let path = archive.path();
            if path
                .extension()
                .is_none_or(|extension| extension != "crate")
            {
                continue;
            }
            let metadata = fs::metadata(&path)?;
            if metadata.is_file() {
                archives.insert(
                    path,
                    Stamp {
                        bytes: metadata.len(),
                        modified: metadata.modified()?,
                    },
                );
            }
        }
    }
    Ok(archives)
}
