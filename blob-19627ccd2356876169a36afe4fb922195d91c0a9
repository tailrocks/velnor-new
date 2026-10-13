//! Read-only tool-input checks shared by plan and generate (tool §1-§2).
//!
//! Asks the owning adapter about Rust toolchain files and reads the
//! declared Mise sources read-only, reporting presence, parse status,
//! extracted values, and content digests for every path. Nothing here writes,
//! repairs, or passes project configuration to execution.

use std::collections::BTreeMap;
use std::path::Path;

use crate::native_tool_input::{
    NativeToolInput, NativeToolSource, flatten_json, native_mise_source, toml_to_json,
};
use velnor_actions_contract::digest_b3;
use velnor_actions_rust::{TOOLING_INPUT_INVALID, inspect_toolchain_file};

/// Tool-input paths checked on every plan and generate.
pub const TOOL_INPUT_PATHS: [&str; 3] = ["rust-toolchain.toml", "mise.toml", "mise.lock"];

/// Maximum problem detail kept from a parse failure.
const MAX_PROBLEM: usize = 120;

/// Parse status of one tool-input file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolParse {
    /// File is absent (`NotFound`); never emitted for IO failures.
    Missing,
    /// File exists but its bytes are inaccessible (P09-7: never
    /// collapsed into [`ToolParse::Missing`]; the no-write proof
    /// needs contents, so absence and inaccessibility differ).
    Unreadable,
    /// File parsed; values extracted where the shape is known.
    Valid,
    /// File is present but malformed.
    Invalid {
        /// First-line problem detail.
        problem: String,
    },
}

/// One checked tool-input file: presence, parse, values, digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInputCheck {
    /// Repository-relative path.
    pub path: String,
    /// True when bytes were readable.
    pub present: bool,
    /// Parse status.
    pub parse: ToolParse,
    /// Extracted `dotted.key` values, sorted.
    pub values: BTreeMap<String, String>,
    /// BLAKE3 digest over raw bytes, when readable.
    pub digest: Option<String>,
    /// Adapter finding codes, sorted unique (TOOL-1.2).
    pub codes: Vec<String>,
    /// Sanitized native-build projection of this source, never raw config.
    pub(crate) native: Option<NativeToolInput>,
}

/// Check every tool-input path under `root`.
#[must_use]
pub fn check_tool_inputs(root: &Path) -> Vec<ToolInputCheck> {
    check_tool_inputs_with_paths(root, &[])
}

/// Check default tooling inputs plus explicitly declared task source files.
#[must_use]
pub(crate) fn check_tool_inputs_with_paths(
    root: &Path,
    additional_paths: &[String],
) -> Vec<ToolInputCheck> {
    let mut paths = TOOL_INPUT_PATHS.map(str::to_owned).to_vec();
    for path in additional_paths {
        if !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    paths
        .iter()
        .map(|path| {
            let text = match crate::safe_read::read_repo_file(
                root,
                path,
                crate::safe_read::MAX_REPO_FILE_BYTES,
            ) {
                Ok(crate::safe_read::RepoRead::Absent) => None,
                Ok(crate::safe_read::RepoRead::Text(text)) => Some(text),
                Err(_) => return inaccessible(path),
            };
            match Path::new(path).file_name().and_then(|name| name.to_str()) {
                Some("rust-toolchain.toml") => check_toolchain(path, text.as_deref()),
                Some("mise.toml") => check_mise_file(path, text.as_deref(), false),
                Some("mise.lock") => check_mise_file(path, text.as_deref(), true),
                _ => inaccessible(path),
            }
        })
        .collect()
}

/// Check for a tool file that exists but cannot be read.
///
/// `present` stays false (no bytes were readable) while the parse
/// status names the inaccessibility, so callers never mistake it
/// for an absent file.
fn inaccessible(path: &str) -> ToolInputCheck {
    ToolInputCheck {
        path: path.to_owned(),
        present: false,
        parse: ToolParse::Unreadable,
        values: BTreeMap::new(),
        digest: None,
        codes: vec![TOOLING_INPUT_INVALID.to_owned()],
        native: None,
    }
}

/// Check the Rust-owned toolchain file through the Rust adapter.
fn check_toolchain(path: &str, content: Option<&str>) -> ToolInputCheck {
    let digest = content.map(|text| digest_b3(text.as_bytes()));
    let Ok(inspection) = inspect_toolchain_file(path, content) else {
        return unreadable(path, digest);
    };
    let parse = if content.is_none() {
        ToolParse::Missing
    } else if let Some(problem) = invalid_problem(&inspection.findings) {
        ToolParse::Invalid {
            problem: shorten(&problem),
        }
    } else {
        ToolParse::Valid
    };
    ToolInputCheck {
        path: path.to_owned(),
        present: content.is_some(),
        parse,
        values: spec_values(inspection.spec.as_ref()),
        digest,
        codes: finding_codes(&inspection.findings),
        native: content.map(|text| NativeToolInput {
            sha256: crate::cover_identity::generator::sha256_hex(text.as_bytes()),
            source: NativeToolSource::RustToolchain,
        }),
    }
}

/// Sorted unique finding codes from adapter findings.
fn finding_codes(findings: &[velnor_actions_rust::ToolFinding]) -> Vec<String> {
    let mut codes: Vec<String> = findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

/// Invalid problem from adapter findings, if the adapter flagged any.
fn invalid_problem(findings: &[velnor_actions_rust::ToolFinding]) -> Option<String> {
    findings.iter().find_map(|finding| {
        (finding.code == TOOLING_INPUT_INVALID).then(|| {
            finding
                .observed
                .clone()
                .unwrap_or_else(|| "malformed".to_owned())
        })
    })
}

/// Extracted channel, components, and targets from an adapter spec.
fn spec_values(spec: Option<&velnor_actions_rust::ToolchainSpec>) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let Some(spec) = spec else {
        return values;
    };
    if let Some(channel) = &spec.channel {
        values.insert("channel".to_owned(), channel.clone());
    }
    if !spec.components.is_empty() {
        values.insert("components".to_owned(), spec.components.join(","));
    }
    if !spec.targets.is_empty() {
        values.insert("targets".to_owned(), spec.targets.join(","));
    }
    values
}

/// Check for bytes that cannot be inspected as text.
fn unreadable(path: &str, digest: Option<String>) -> ToolInputCheck {
    ToolInputCheck {
        path: path.to_owned(),
        present: true,
        parse: ToolParse::Invalid {
            problem: "unreadable_utf8".to_owned(),
        },
        values: BTreeMap::new(),
        digest,
        codes: vec![TOOLING_INPUT_INVALID.to_owned()],
        native: None,
    }
}

/// Check a Mise-owned file read-only: TOML shape plus flattened values.
///
/// Lockfiles also accept JSON shape; their values stay scalar leaves
/// only because lock internals belong to the Mise adapter.
fn check_mise_file(path: &str, text: Option<&str>, json_fallback: bool) -> ToolInputCheck {
    let digest = text.map(|value| digest_b3(value.as_bytes()));
    let Some(text) = text else {
        return ToolInputCheck {
            path: path.to_owned(),
            present: false,
            parse: ToolParse::Missing,
            values: BTreeMap::new(),
            digest,
            codes: Vec::new(),
            native: None,
        };
    };
    match toml::from_str::<toml::Value>(text) {
        Ok(value) => {
            let native = native_mise_source(path, text.as_bytes(), &value);
            ToolInputCheck {
                path: path.to_owned(),
                present: true,
                parse: ToolParse::Valid,
                values: flatten_json(&toml_to_json(&value)),
                digest,
                codes: Vec::new(),
                native,
            }
        }
        Err(toml_err) => {
            if json_fallback && let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                return ToolInputCheck {
                    path: path.to_owned(),
                    present: true,
                    parse: ToolParse::Valid,
                    values: flatten_json(&json),
                    digest,
                    codes: Vec::new(),
                    native: None,
                };
            }
            ToolInputCheck {
                path: path.to_owned(),
                present: true,
                parse: ToolParse::Invalid {
                    problem: shorten(&toml_err.to_string()),
                },
                values: BTreeMap::new(),
                digest,
                codes: vec![TOOLING_INPUT_INVALID.to_owned()],
                native: None,
            }
        }
    }
}

/// First line of `problem`, truncated to [`MAX_PROBLEM`] chars.
fn shorten(problem: &str) -> String {
    let first = problem.lines().next().unwrap_or("malformed");
    let mut short: String = first.chars().take(MAX_PROBLEM).collect();
    if first.len() > MAX_PROBLEM {
        short.push_str("...");
    }
    short
}

#[cfg(test)]
#[path = "toolcheck_tests.rs"]
mod tests;
