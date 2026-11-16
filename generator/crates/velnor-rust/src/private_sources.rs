// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView, RustRoot};

use crate::roots::{CARGO_LOCK, is_cargo_config};
use crate::toml_file::table;

const PUBLIC_SOURCES: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

pub(crate) fn private_sources(view: &RepoView, roots: &[RustRoot]) -> Option<String> {
    let locks = roots
        .iter()
        .map(|root| root.manifest.sibling(&CARGO_LOCK))
        .filter_map(|lock| {
            view.files
                .get(&lock)
                .map(|bytes| locked_source(&lock, bytes))
        })
        .flatten();
    let configs = view
        .files
        .iter()
        .filter(|(path, _)| is_cargo_config(path))
        .filter_map(|(path, bytes)| configured_source(path, bytes));
    locks.chain(configs).next().map(|cause| {
        format!(
            "private or non-crates.io dependencies ({cause}): saves run only in private repositories, so nothing built from them reaches a cache a fork can read"
        )
    })
}

fn locked_source(lock: &RepoPath, bytes: &[u8]) -> Option<String> {
    let Ok(document) = table(bytes) else {
        return Some(format!("{lock} is unreadable"));
    };
    document
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .find_map(|package| {
            let source = package.get("source").and_then(toml::Value::as_str)?;
            let name = package.get("name").and_then(toml::Value::as_str)?;
            (!PUBLIC_SOURCES.contains(&source))
                .then(|| format!("{lock} locks {name} from {source}"))
        })
}

fn configured_source(path: &RepoPath, bytes: &[u8]) -> Option<String> {
    let Ok(document) = table(bytes) else {
        return None;
    };
    ["registries", "source"]
        .into_iter()
        .find(|section| {
            document
                .get(*section)
                .and_then(toml::Value::as_table)
                .is_some_and(|entries| !entries.is_empty())
        })
        .map(|section| format!("{path} declares [{section}]"))
}
