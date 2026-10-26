// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dialect {
    Native,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigShape {
    pub stem: String,
    pub dialect: Dialect,
    pub tofu_spelling: bool,
}

#[must_use]
pub fn config_shape(file_name: &str) -> Option<ConfigShape> {
    for (suffix, dialect, tofu_spelling) in [
        (".tofu.json", Dialect::Json, true),
        (".tf.json", Dialect::Json, false),
        (".tofu", Dialect::Native, true),
        (".tf", Dialect::Native, false),
    ] {
        if let Some(stem) = file_name.strip_suffix(suffix)
            && !stem.is_empty()
        {
            return Some(ConfigShape {
                stem: stem.to_owned(),
                dialect,
                tofu_spelling,
            });
        }
    }
    None
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[must_use]
pub fn effective_set(paths: &[String]) -> Vec<String> {
    let mut best: BTreeMap<(String, String, Dialect), (bool, &str)> = BTreeMap::new();
    for path in paths {
        let Some(shape) = config_shape(file_name(path)) else {
            continue;
        };
        let key = (parent_dir(path).to_owned(), shape.stem, shape.dialect);
        best.entry(key)
            .and_modify(|slot| {
                if shape.tofu_spelling && !slot.0 {
                    *slot = (true, path.as_str());
                }
            })
            .or_insert((shape.tofu_spelling, path.as_str()));
    }
    let mut survivors: Vec<String> = best.values().map(|slot| slot.1.to_owned()).collect();
    survivors.sort();
    survivors
}

#[must_use]
pub fn dir_files(files: &[String], prefix: &str) -> Vec<String> {
    files
        .iter()
        .filter(|path| parent_dir(path) == prefix)
        .cloned()
        .collect()
}

#[must_use]
pub fn effective_in_dir(files: &[String], prefix: &str) -> Vec<String> {
    effective_set(&dir_files(files, prefix))
}

#[must_use]
pub fn dir_has_effective_config(files: &[String], prefix: &str) -> bool {
    !effective_in_dir(files, prefix).is_empty()
}
