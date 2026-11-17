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
    pub(super) fn start(argv: &[String]) -> Self {
        if cannot_download(argv) {
            return Self {
                cache: None,
                before: Some(BTreeMap::new()),
            };
        }
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
        let Some(cache) = self.cache else {
            return Some(Vec::new());
        };
        let after = scan(&cache).ok()?;
        let changed = after
            .into_iter()
            .filter(|(path, stamp)| before.get(path) != Some(stamp));
        changed
            .map(|(path, _)| path.strip_prefix(&cache).ok()?.to_str().map(str::to_owned))
            .collect::<Option<BTreeSet<_>>>()
            .map(|names| names.into_iter().collect())
    }
}

pub(super) fn cannot_download(argv: &[String]) -> bool {
    if argv
        .first()
        .and_then(|program| Path::new(program).file_name())
        != Some("cargo".as_ref())
    {
        return false;
    }
    let mut words = argv.iter().skip(1).peekable();
    if words.peek().is_some_and(|word| word.starts_with('+')) {
        words.next();
    }
    let mut command = None;
    let mut offline = false;
    while let Some(word) = words.next() {
        if word == "--" {
            break;
        }
        let flag = word.split_once('=').map_or(word.as_str(), |(flag, _)| flag);
        match flag {
            "--offline" | "--frozen" if flag == word => offline = true,
            "--config" | "--color" | "--manifest-path" | "--lockfile-path" | "--target-dir"
            | "--target" | "--profile" | "--features" | "--package" | "--exclude" | "--bin"
            | "--example" | "--test" | "--bench" | "--jobs" | "--message-format" | "--timings"
            | "-C" | "-Z" | "-F" | "-p" | "-j" => {
                if !word.contains('=') && words.next().is_none() {
                    return false;
                }
            }
            "--locked"
            | "--release"
            | "--workspace"
            | "--all"
            | "--all-targets"
            | "--lib"
            | "--bins"
            | "--examples"
            | "--tests"
            | "--benches"
            | "--all-features"
            | "--no-default-features"
            | "--no-deps"
            | "--no-run"
            | "--quiet"
            | "--verbose"
            | "-q"
            | "-v"
            | "-vv"
            | "-r"
                if flag == word => {}
            "fmt" if command.is_none() => return true,
            "run" | "list" if command == Some("nextest") => {}
            _ if !word.starts_with('-') && command.is_none() => command = Some(word.as_str()),
            _ => return false,
        }
    }
    match command {
        Some("fmt") => true,
        Some("fetch" | "metadata") => offline,
        _ => false,
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
