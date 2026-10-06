//! Read-only tool-input checks shared by plan and generate (tool §1-§2).
//!
//! Asks the owning adapter about `rust-toolchain.toml` and reads the
//! Mise files read-only, reporting presence, parse status, extracted
//! values, and content digests for every path. Nothing here writes,
//! repairs, or passes project configuration to execution.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::digest_b3;
use velnor_actions_rust_core::{TOOLING_INPUT_INVALID, inspect_toolchain_file};

/// Tool-input paths checked on every plan and generate.
pub const TOOL_INPUT_PATHS: [&str; 3] = ["rust-toolchain.toml", "mise.toml", "mise.lock"];

/// Maximum flattened values kept per tool file.
const MAX_VALUES: usize = 64;
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
}

/// Check every tool-input path under `root`.
#[must_use]
pub fn check_tool_inputs(root: &Path) -> Vec<ToolInputCheck> {
    TOOL_INPUT_PATHS
        .iter()
        .map(|path| {
            // P09-7: `NotFound` is missing; any other IO failure is
            // unreadable. `.ok()` would collapse the two, hiding an
            // inaccessible file behind a "not found" diagnosis.
            let read = match std::fs::read(root.join(path)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(()),
            };
            match read {
                Err(()) => inaccessible(path),
                Ok(bytes) => {
                    if *path == TOOL_INPUT_PATHS[0] {
                        check_toolchain(path, bytes.as_deref())
                    } else {
                        check_mise_file(path, bytes.as_deref(), *path == TOOL_INPUT_PATHS[2])
                    }
                }
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
    }
}

/// Check the Rust-owned toolchain file through the Rust adapter.
fn check_toolchain(path: &str, bytes: Option<&[u8]>) -> ToolInputCheck {
    let digest = bytes.map(digest_b3);
    let content = bytes.and_then(|raw| std::str::from_utf8(raw).ok());
    if bytes.is_some() && content.is_none() {
        return unreadable(path, digest);
    }
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
    }
}

/// Sorted unique finding codes from adapter findings.
fn finding_codes(findings: &[velnor_actions_rust_core::ToolFinding]) -> Vec<String> {
    let mut codes: Vec<String> = findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

/// Invalid problem from adapter findings, if the adapter flagged any.
fn invalid_problem(findings: &[velnor_actions_rust_core::ToolFinding]) -> Option<String> {
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
fn spec_values(spec: Option<&velnor_actions_rust_core::ToolchainSpec>) -> BTreeMap<String, String> {
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
    }
}

/// Check a Mise-owned file read-only: TOML shape plus flattened values.
///
/// Lockfiles also accept JSON shape; their values stay scalar leaves
/// only because lock internals belong to the Mise adapter.
fn check_mise_file(path: &str, bytes: Option<&[u8]>, json_fallback: bool) -> ToolInputCheck {
    let digest = bytes.map(digest_b3);
    let Some(raw) = bytes else {
        return ToolInputCheck {
            path: path.to_owned(),
            present: false,
            parse: ToolParse::Missing,
            values: BTreeMap::new(),
            digest,
            codes: Vec::new(),
        };
    };
    let Ok(text) = std::str::from_utf8(raw) else {
        return unreadable(path, digest);
    };
    match toml::from_str::<toml::Value>(text) {
        Ok(value) => ToolInputCheck {
            path: path.to_owned(),
            present: true,
            parse: ToolParse::Valid,
            values: flatten_json(&toml_to_json(&value)),
            digest,
            codes: Vec::new(),
        },
        Err(toml_err) => {
            if json_fallback && let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                return ToolInputCheck {
                    path: path.to_owned(),
                    present: true,
                    parse: ToolParse::Valid,
                    values: flatten_json(&json),
                    digest,
                    codes: Vec::new(),
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
            }
        }
    }
}

/// Convert TOML to JSON so one flattener serves both shapes.
fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

/// Flatten scalar leaves into dotted keys, capped and sorted.
fn flatten_json(value: &serde_json::Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    flatten_into(String::new(), value, &mut out);
    out
}

/// Recurse one value, recording scalar leaves only.
fn flatten_into(prefix: String, value: &serde_json::Value, out: &mut BTreeMap<String, String>) {
    if out.len() >= MAX_VALUES {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let scoped = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_into(scoped, child, out);
            }
        }
        serde_json::Value::Array(items) => {
            let scalars: Vec<String> = items.iter().filter_map(json_scalar).collect();
            if !prefix.is_empty() && !scalars.is_empty() {
                out.insert(prefix, scalars.join(","));
            }
        }
        _ => {
            if !prefix.is_empty()
                && let Some(scalar) = json_scalar(value)
            {
                out.insert(prefix, scalar);
            }
        }
    }
}

/// Scalar text of one JSON value, if it is a scalar.
fn json_scalar(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(num) => Some(num.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            None
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
mod tests;
