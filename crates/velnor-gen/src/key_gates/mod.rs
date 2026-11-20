// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! One table of the config keys `velnor-ci` reads, each with the first release that parses it.
//!
//! A `velnor-ci` older than a key's row fails on the repository's config at run time, so
//! generation refuses the pin when the key is written. A test walks the parsers' own key lists
//! and fails when a key has no row, so a new key cannot land without a gate.

mod release;
mod rust_ci;

use velnor_model::{HelperVersion, RELEASE_CONFIG_PATH, RepoPath, RepoView};
use velnor_rust::CONFIG_PATH;

use crate::GenerateError;

/// The config file a key lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFile {
    RustCi,
    Release,
}

impl ConfigFile {
    #[must_use]
    pub fn path(self) -> RepoPath {
        match self {
            Self::RustCi => CONFIG_PATH,
            Self::Release => RELEASE_CONFIG_PATH,
        }
    }
}

/// A dotted key path (array elements are transparent: `lint.kind` is the `kind` of each
/// `[[lint]]`) and the first `velnor-ci` whose parser accepts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyGate {
    pub file: ConfigFile,
    pub key: &'static str,
    pub value: Option<&'static str>,
    pub first: (u64, u64, u64),
}

const fn gate(file: ConfigFile, key: &'static str, first: (u64, u64, u64)) -> KeyGate {
    KeyGate {
        file,
        key,
        value: None,
        first,
    }
}

const fn gate_value(
    file: ConfigFile,
    key: &'static str,
    value: &'static str,
    first: (u64, u64, u64),
) -> KeyGate {
    KeyGate {
        file,
        key,
        value: Some(value),
        first,
    }
}

/// Every row, `rust-ci.toml` first.
pub fn key_gates() -> impl Iterator<Item = &'static KeyGate> {
    rust_ci::RUST_CI.iter().chain(release::RELEASE)
}

fn written(value: &toml::Value, segments: &[&str], expected: Option<&str>) -> bool {
    match (value, segments.split_first()) {
        (_, None) => expected.is_none_or(|wanted| value.as_str() == Some(wanted)),
        (toml::Value::Table(table), Some((head, rest))) => table
            .get(*head)
            .is_some_and(|inner| written(inner, rest, expected)),
        (toml::Value::Array(items), _) => items
            .iter()
            .any(|item| written(item, segments, expected)),
        _ => false,
    }
}

/// The row with the newest `first` among the keys the repository's config writes, ignoring the
/// rows `skip` accepts.
///
/// Presence decides, not the value: an old `velnor-ci` rejects `sparse_checkout = false` too.
fn newest(view: &RepoView, skip: impl Fn(&KeyGate) -> bool) -> Option<&'static KeyGate> {
    let mut newest: Option<&KeyGate> = None;
    for file in [ConfigFile::RustCi, ConfigFile::Release] {
        let Some(bytes) = view.files.get(&file.path()) else {
            continue;
        };
        let Ok(table) = std::str::from_utf8(bytes)
            .map_err(drop)
            .and_then(|text| toml::from_str::<toml::Table>(text).map_err(drop))
        else {
            continue;
        };
        let root = toml::Value::Table(table);
        for row in key_gates().filter(|row| row.file == file && !skip(row)) {
            let segments: Vec<&str> = row.key.split('.').collect();
            if written(&root, &segments, row.value) && newest.is_none_or(|best| row.first > best.first) {
                newest = Some(row);
            }
        }
    }
    newest
}

/// The first `velnor-ci` that parses every key the repository's config writes.
#[must_use]
pub fn required_helper(view: &RepoView) -> Option<(u64, u64, u64)> {
    newest(view, |_| false).map(|row| row.first)
}

/// Refuses a pin older than the newest key the repository's config writes.
pub fn check_config_keys(view: &RepoView, pinned: &HelperVersion) -> Result<(), GenerateError> {
    match newest(view, |row| pinned.at_least(row.first)) {
        Some(row) => Err(GenerateError::ConfigKeyHelperTooOld {
            file: row.file.path().to_string(),
            key: row.key,
            value: row.value,
            first: row.first,
            pinned: pinned.clone(),
        }),
        None => Ok(()),
    }
}
