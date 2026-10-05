//! Configured-unit analysis: effective set, structural validity, scopes.
//!
//! The caller reads file bytes (bounded); this module never touches
//! the filesystem. Shadowed files are dropped BEFORE parsing (never
//! read, never parsed). Structural failures yield a typed
//! [`UnitError`]: HCL/JSON syntax, non-object JSON roots, unknown
//! top-level block types, bad label counts on tracked kinds, and
//! duplicate declarations across non-override effective files in one
//! directory (both dialects share one identity space: HCL+JSON
//! doubles error; child directories keep their own namespace).
//! Override files are exempt from duplicate detection (repeating
//! blocks is their purpose) but not from block-type checks.

use std::collections::BTreeMap;
use std::fmt;

use crate::effective::{Dialect, config_shape, effective_set};
use crate::family::{Family, family_of};
use crate::fmt_scope::fmt_scope_for_root;
use crate::modules::ModuleRef;
use crate::parser::{MAX_FILES_PER_UNIT, parse_json, parse_native};

/// Top-level block types accepted in config files (superset-safe:
/// validate judges anything subtler).
const KNOWN_TOP_LEVEL: [&str; 13] = [
    "terraform",
    "resource",
    "data",
    "variable",
    "output",
    "locals",
    "module",
    "provider",
    "import",
    "moved",
    "removed",
    "check",
    "ephemeral",
];

/// Tracked declaration kinds with their exact label counts.
const TRACKED_LABELS: [(&str, usize); 5] = [
    ("variable", 1),
    ("output", 1),
    ("module", 1),
    ("resource", 2),
    ("data", 2),
];

/// Analyzed unit: load set plus independent fmt scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedUnit {
    /// Sorted effective load-set survivors (shadowed files dropped).
    pub effective: Vec<String>,
    /// Sorted fmt scope (precedence-independent, shadowed included).
    pub fmt: Vec<String>,
    /// Module references from effective files (unresolved; boundary qualifies).
    pub modules: Vec<ModuleRef>,
}

/// Typed unit-analysis failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitError {
    /// Unit holds more files than [`MAX_FILES_PER_UNIT`].
    TooManyFiles {
        /// Observed file count.
        count: usize,
    },
    /// One effective file failed to parse.
    Parse {
        /// Failing file.
        path: String,
        /// Capped parser message.
        message: String,
    },
    /// One effective file carries an unknown top-level block type.
    UnknownBlock {
        /// Failing file.
        path: String,
        /// Unknown block type.
        kind: String,
    },
    /// A tracked block carries a wrong label count.
    Shape {
        /// Failing file.
        path: String,
        /// Stable shape diagnostic.
        message: String,
    },
    /// A tracked declaration repeats across non-override files.
    Duplicate {
        /// Repeated block type.
        kind: String,
        /// Repeated labels.
        labels: Vec<String>,
        /// First declaration file.
        first: String,
        /// Offending repeat file.
        second: String,
    },
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyFiles { count } => write!(f, "too_many_files:{count}"),
            Self::Parse { path, message } => write!(f, "malformed:{path}:{message}"),
            Self::UnknownBlock { path, kind } => {
                write!(f, "unknown_block:{path}:{kind}")
            }
            Self::Shape { path, message } => write!(f, "bad_shape:{path}:{message}"),
            Self::Duplicate {
                kind,
                labels,
                first,
                second,
            } => write!(
                f,
                "duplicate:{}:{}:{first}:{second}",
                kind,
                labels.join(",")
            ),
        }
    }
}

impl std::error::Error for UnitError {}

/// Repo-relative files under `prefix`, capped (`""` = whole index).
///
/// # Errors
///
/// Returns [`UnitError::TooManyFiles`] when the selection exceeds
/// [`MAX_FILES_PER_UNIT`].
pub fn files_for_prefix(files: &[String], prefix: &str) -> Result<Vec<String>, UnitError> {
    let selected: Vec<String> = files
        .iter()
        .filter(|path| prefix.is_empty() || path.starts_with(&format!("{prefix}/")))
        .cloned()
        .collect();
    if selected.len() > MAX_FILES_PER_UNIT {
        return Err(UnitError::TooManyFiles {
            count: selected.len(),
        });
    }
    Ok(selected)
}

/// Module references for `(path, bounded text)` config pairs.
///
/// Parses without block-type, shape, or duplicate checks: base-graph
/// selection needs references from history that predates validation.
/// Every pair must be an effective-shape config file.
///
/// # Errors
///
/// Returns [`UnitError`] for over-cap selections, non-config paths,
/// and parse failures.
pub fn module_refs_for_texts(pairs: &[(String, String)]) -> Result<Vec<ModuleRef>, UnitError> {
    if pairs.len() > MAX_FILES_PER_UNIT {
        return Err(UnitError::TooManyFiles { count: pairs.len() });
    }
    let mut refs = Vec::new();
    for (path, text) in pairs {
        let name = path.rsplit('/').next().unwrap_or(path);
        let shape = config_shape(name).ok_or_else(|| UnitError::Parse {
            path: path.clone(),
            message: "not_config".to_owned(),
        })?;
        let model = match shape.dialect {
            Dialect::Native => parse_native(text),
            Dialect::Json => parse_json(text),
        }
        .map_err(|err| UnitError::Parse {
            path: path.clone(),
            message: err.to_string(),
        })?;
        for decl in &model.modules {
            refs.push(ModuleRef {
                file: path.clone(),
                name: decl.name.clone(),
                source: decl.source.clone(),
            });
        }
    }
    Ok(refs)
}

/// Analyze one unit's `(path, bounded text)` pairs.
///
/// # Errors
///
/// Returns [`UnitError`] for over-cap selections and every
/// structural failure above.
pub fn analyze_files(pairs: &[(String, String)]) -> Result<AnalyzedUnit, UnitError> {
    if pairs.len() > MAX_FILES_PER_UNIT {
        return Err(UnitError::TooManyFiles { count: pairs.len() });
    }
    let paths: Vec<String> = pairs.iter().map(|(path, _)| path.clone()).collect();
    let effective = effective_set(&paths);
    let by_path: BTreeMap<&str, &str> = pairs
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut by_dir: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for path in &effective {
        let dir = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        by_dir.entry(dir).or_default().push(path.as_str());
    }
    let mut modules = Vec::new();
    for group in by_dir.values() {
        let mut seen: BTreeMap<(String, Vec<String>), String> = BTreeMap::new();
        for path in group {
            let name = path.rsplit('/').next().unwrap_or(path);
            let shape = config_shape(name).ok_or_else(|| UnitError::Parse {
                path: (*path).to_owned(),
                message: "not_config".to_owned(),
            })?;
            let text = by_path.get(path).ok_or_else(|| UnitError::Parse {
                path: (*path).to_owned(),
                message: "missing_text".to_owned(),
            })?;
            let model = match shape.dialect {
                Dialect::Native => parse_native(text),
                Dialect::Json => parse_json(text),
            }
            .map_err(|err| UnitError::Parse {
                path: (*path).to_owned(),
                message: err.to_string(),
            })?;
            check_blocks(
                path,
                &model.blocks,
                family_of(name) == Family::Override,
                &mut seen,
            )?;
            for decl in &model.modules {
                modules.push(ModuleRef {
                    file: (*path).to_owned(),
                    name: decl.name.clone(),
                    source: decl.source.clone(),
                });
            }
        }
    }
    Ok(AnalyzedUnit {
        fmt: fmt_scope_for_root(&paths, ""),
        effective,
        modules,
    })
}

/// Check block types, label shapes, and cross-file duplicates.
fn check_blocks(
    path: &str,
    blocks: &[crate::parser::BlockModel],
    is_override: bool,
    seen: &mut BTreeMap<(String, Vec<String>), String>,
) -> Result<(), UnitError> {
    for block in blocks {
        if !KNOWN_TOP_LEVEL.contains(&block.kind.as_str()) {
            return Err(UnitError::UnknownBlock {
                path: path.to_owned(),
                kind: block.kind.clone(),
            });
        }
        let Some((_, want)) = TRACKED_LABELS.iter().find(|(kind, _)| *kind == block.kind) else {
            continue;
        };
        if block.labels.len() != *want {
            return Err(UnitError::Shape {
                path: path.to_owned(),
                message: format!(
                    "{}_wants_{want}_labels_got_{}",
                    block.kind,
                    block.labels.len()
                ),
            });
        }
        if is_override {
            continue;
        }
        let key = (block.kind.clone(), block.labels.clone());
        if let Some(first) = seen.get(&key) {
            return Err(UnitError::Duplicate {
                kind: block.kind.clone(),
                labels: block.labels.clone(),
                first: first.clone(),
                second: path.to_owned(),
            });
        }
        seen.insert(key, path.to_owned());
    }
    Ok(())
}
