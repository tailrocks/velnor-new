//! Effective config files per S1 precedence (T09 minimal).
//!
//! Config suffixes are `.tf`/`.tofu` (native dialect) and
//! `.tf.json`/`.tofu.json` (JSON dialect). Override files count: they
//! carry these same suffixes. Test (`.tftest.hcl`) and var
//! (`.tfvars`) files are never configuration. Within one directory,
//! `.tofu` shadows `.tf` and `.tofu.json` shadows `.tf.json` per
//! (basename, dialect) group; native and JSON never suppress each
//! other.

use std::collections::BTreeMap;

/// HCL dialect of one config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dialect {
    /// `.tf` / `.tofu`.
    Native,
    /// `.tf.json` / `.tofu.json`.
    Json,
}

/// Shape of one config file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigShape {
    /// Base name with the config suffix stripped (never empty).
    pub stem: String,
    /// File dialect.
    pub dialect: Dialect,
    /// True for the `.tofu*` spellings (shadowing rank).
    pub tofu_spelling: bool,
}

/// Shape of `file_name`, or `None` when not a config file.
///
/// Matches longest suffix first so `.tf.json` never misreads as
/// native; names that are bare suffixes (empty stem) do not count.
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

/// Parent directory of a repo-relative path; empty for the root.
fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// File name of a repo-relative path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Sorted effective survivors among repo-relative `paths`.
///
/// Groups by (directory, stem, dialect) and keeps the shadowing
/// rank winner per group; non-config files never enter a group.
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

/// Sorted direct-child files of `prefix` (`""` = repository root).
#[must_use]
pub fn dir_files(files: &[String], prefix: &str) -> Vec<String> {
    files
        .iter()
        .filter(|path| parent_dir(path) == prefix)
        .cloned()
        .collect()
}

/// Sorted effective survivors directly inside `prefix`.
#[must_use]
pub fn effective_in_dir(files: &[String], prefix: &str) -> Vec<String> {
    effective_set(&dir_files(files, prefix))
}

/// Whether `prefix` holds at least one effective config file.
#[must_use]
pub fn dir_has_effective_config(files: &[String], prefix: &str) -> bool {
    !effective_in_dir(files, prefix).is_empty()
}
