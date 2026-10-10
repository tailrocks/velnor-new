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
mod tests {
    use super::*;

    #[test]
    fn checks_report_presence_parse_values_and_digests() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.85.0\"\ncomponents = [\"clippy\"]\n",
        )
        .expect("write");
        std::fs::write(dir.path().join("mise.toml"), "[tools]\nrust = \"1.85.0\"\n")
            .expect("write");
        std::fs::write(dir.path().join("mise.lock"), "{}\n").expect("write");
        let checks = check_tool_inputs(dir.path());
        assert_eq!(checks.len(), 3);
        for check in &checks {
            assert!(check.present, "{}", check.path);
            assert_eq!(check.parse, ToolParse::Valid, "{}", check.path);
            assert!(
                check
                    .digest
                    .as_deref()
                    .is_some_and(|d| d.starts_with("b3-"))
            );
        }
        let toolchain = checks
            .iter()
            .find(|c| c.path == "rust-toolchain.toml")
            .expect("t");
        assert_eq!(
            toolchain.values.get("channel").map(String::as_str),
            Some("1.85.0")
        );
        let mise = checks.iter().find(|c| c.path == "mise.toml").expect("m");
        assert_eq!(
            mise.values.get("tools.rust").map(String::as_str),
            Some("1.85.0")
        );
        let missing = check_tool_inputs(std::path::Path::new("/nonexistent-root-velnor"));
        assert!(missing.iter().all(|check| !check.present));
        assert!(
            missing
                .iter()
                .all(|check| check.parse == ToolParse::Missing)
        );
    }

    #[test]
    fn declared_nested_mise_sources_are_read_and_hashed_by_exact_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let native = dir.path().join("native");
        std::fs::create_dir(&native).expect("native dir");
        let config = "[tasks.desktop-format-check]\nrun = \"echo format\"\n";
        let lock = "[tools]\n";
        let toolchain = "[toolchain]\nchannel = \"1.99.0\"\n";
        std::fs::write(native.join("mise.toml"), config).expect("native mise config");
        std::fs::write(native.join("mise.lock"), lock).expect("native mise lock");
        std::fs::write(native.join("rust-toolchain.toml"), toolchain)
            .expect("native rust toolchain");

        let paths = [
            "native/mise.toml".to_owned(),
            "native/mise.lock".to_owned(),
            "native/rust-toolchain.toml".to_owned(),
        ];
        let checks = check_tool_inputs_with_paths(dir.path(), &paths);
        assert_eq!(checks.len(), 6, "root inputs plus three declared sources");
        let config_check = checks
            .iter()
            .find(|check| check.path == "native/mise.toml")
            .expect("nested config check");
        assert_eq!(config_check.parse, ToolParse::Valid);
        assert!(config_check.native.is_some());
        assert_eq!(
            config_check
                .values
                .get("tasks.desktop-format-check.run")
                .map(String::as_str),
            Some("echo format")
        );
        let lock_check = checks
            .iter()
            .find(|check| check.path == "native/mise.lock")
            .expect("nested lock check");
        assert_eq!(lock_check.parse, ToolParse::Valid);
        assert!(lock_check.native.is_some());
        let rust_check = checks
            .iter()
            .find(|check| check.path == "native/rust-toolchain.toml")
            .expect("nested toolchain check");
        assert_eq!(rust_check.parse, ToolParse::Valid);
        assert_eq!(
            rust_check.values.get("channel").map(String::as_str),
            Some("1.99.0")
        );
    }

    #[test]
    fn malformed_files_are_invalid_with_digests() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("mise.toml"), "[tools\nrust = \n").expect("write");
        let checks = check_tool_inputs(dir.path());
        let mise = checks.iter().find(|c| c.path == "mise.toml").expect("m");
        assert!(matches!(mise.parse, ToolParse::Invalid { .. }));
        assert!(mise.digest.is_some());
    }

    #[test]
    fn unreadable_files_are_not_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A directory where the tool file belongs: `read` fails with a
        // non-`NotFound` IO error on every platform, even for root.
        std::fs::create_dir(dir.path().join("mise.toml")).expect("dir");
        let checks = check_tool_inputs(dir.path());
        let mise = checks.iter().find(|c| c.path == "mise.toml").expect("m");
        assert!(!mise.present, "no bytes were readable");
        assert_eq!(
            mise.parse,
            ToolParse::Unreadable,
            "inaccessible, not absent"
        );
        assert!(mise.digest.is_none(), "no digest without bytes");
        assert!(
            mise.codes.contains(&TOOLING_INPUT_INVALID.to_owned()),
            "flagged: {:?}",
            mise.codes
        );
        let lock = checks.iter().find(|c| c.path == "mise.lock").expect("l");
        assert_eq!(lock.parse, ToolParse::Missing, "absent stays missing");
        let lines = crate::toolfindings::tool_check_lines(&checks);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("mise.toml") && line.contains("unreadable")),
            "unreadable surfaces a line: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("mise.lock")),
            "missing stays silent: {lines:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_tool_input_is_rejected_without_following() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        std::fs::write(
            outside.path().join("mise.toml"),
            "[tools]\nrust = \"1.85.0\"\n",
        )
        .expect("write outside tool config");
        std::os::unix::fs::symlink(
            outside.path().join("mise.toml"),
            dir.path().join("mise.toml"),
        )
        .expect("symlink tool config");

        let checks = check_tool_inputs(dir.path());
        let mise = checks
            .iter()
            .find(|check| check.path == "mise.toml")
            .expect("mise");
        assert_eq!(mise.parse, ToolParse::Unreadable);
        assert!(mise.native.is_none());
        assert!(mise.digest.is_none());
    }

    #[test]
    fn oversized_tool_input_is_rejected_before_parsing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let max = usize::try_from(crate::safe_read::MAX_REPO_FILE_BYTES).expect("limit fits");
        let bytes = vec![b'x'; max + 1];
        std::fs::write(dir.path().join("mise.lock"), bytes).expect("write oversized lock");

        let checks = check_tool_inputs(dir.path());
        let lock = checks
            .iter()
            .find(|check| check.path == "mise.lock")
            .expect("lock");
        assert_eq!(lock.parse, ToolParse::Unreadable);
        assert!(lock.native.is_none());
        assert!(lock.digest.is_none());
    }
}
