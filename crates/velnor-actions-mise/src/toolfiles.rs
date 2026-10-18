//! Read-only `mise.toml`/`mise.lock` inspection owned by the Mise adapter.
//!
//! Pure over bytes the orchestrator supplies: this module never reads,
//! writes, or stats the filesystem. Findings always name the supplying
//! file; missing or malformed files yield findings plus manual
//! recommendations, never repairs (tooling-input §1-§3). Reported tool
//! selectors are observations only and never become catalog pins.
//!
//! Install-verification audit lives in [`lockfile`] (declared here so
//! `lib.rs` stays untouched).
#[path = "mise_lockfile.rs"]
pub mod lockfile;

use std::collections::BTreeMap;
use std::fmt;

use crate::catalog::{PinnedTool, validate_exact_version};
use crate::toml_scan::{TomlValue, parse_toml};
use crate::wrappers::parse_cargo_wrapper;

/// Tool-file name owned by the Mise adapter.
pub const MISE_TOML_FILE: &str = "mise.toml";
/// Hidden tool-file name mise also loads; scanned like the plain one.
pub const DOT_MISE_TOML_FILE: &str = ".mise.toml";
/// Lock-file name owned by the Mise adapter.
pub const MISE_LOCK_FILE: &str = "mise.lock";

/// File symbols routed to the Mise stack (quality §1).
pub const OWNED_SYMBOLS: &[&str] = &["mise.toml", ".mise.toml", "mise.lock", ".mise.lock"];

/// Tool files owned by sibling adapters; recognized but never inspected here.
pub const FOREIGN_TOOL_FILES: &[&str] = &["rust-toolchain.toml", "Cargo.toml", "Cargo.lock"];

/// Env-symbol prefix owned by the Mise adapter.
pub const MISE_ENV_PREFIX: &str = "MISE_";

/// Stable code for a missing recommended tool input (tooling-input §1).
pub const MISSING_RECOMMENDED_INPUT: &str = "missing_recommended_input";

/// Stable code for a malformed optional tool file (tooling-input §2).
pub const TOOLING_INPUT_INVALID: &str = "tooling_input_invalid";

/// One named tool-file blob offered for inspection.
#[derive(Debug, Clone, Copy)]
pub struct ToolFile<'a> {
    /// Repository-relative path the content was read from.
    pub path: &'a str,
    /// File content.
    pub content: &'a str,
}

/// Structured tool-input finding with a manual recommendation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFinding {
    /// Stable finding code.
    pub code: String,
    /// Repository-relative file that supplied the finding.
    pub file: String,
    /// Observed value or problem detail, when any.
    pub observed: Option<String>,
    /// Concrete manual action; Velnor never executes it.
    pub recommendation: String,
}

/// Extracted `[tools]` selections (reported values only, never pins).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MiseSpec {
    /// Tool name to declared selector, sorted by name.
    pub tools: BTreeMap<String, String>,
}

/// Outcome of one owned tool-file inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseInspection {
    /// Inspected repository-relative file.
    pub file: String,
    /// Extracted selection (`None` when missing or malformed).
    pub spec: Option<MiseSpec>,
    /// Findings plus manual recommendations; empty when well-formed.
    pub findings: Vec<ToolFinding>,
}

impl MiseInspection {
    /// Inspection for a missing tool file: finding, no spec, no failure.
    fn missing(path: &str) -> Self {
        Self {
            file: path.to_owned(),
            spec: None,
            findings: vec![ToolFinding {
                code: MISSING_RECOMMENDED_INPUT.to_owned(),
                file: path.to_owned(),
                observed: None,
                recommendation: format!(
                    "add a user-authored [tools] table in a new {path}; Velnor never creates it"
                ),
            }],
        }
    }

    /// Inspection for a malformed tool file: finding, no spec, no failure.
    fn invalid(path: &str, problem: &str) -> Self {
        Self {
            file: path.to_owned(),
            spec: None,
            findings: vec![ToolFinding {
                code: TOOLING_INPUT_INVALID.to_owned(),
                file: path.to_owned(),
                observed: Some(problem.to_owned()),
                recommendation: format!(
                    "fix {problem} manually in {path}; generation continues with Velnor pins"
                ),
            }],
        }
    }

    /// Inspection for a readable tool file; selectors stay observations.
    fn present(path: &str, spec: MiseSpec) -> Self {
        Self {
            file: path.to_owned(),
            spec: Some(spec),
            findings: Vec::new(),
        }
    }
}

/// Tool-file inspection failure (ownership only; content issues are findings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolInspectError {
    /// Path belongs to another adapter or to no adapter.
    NotOwned {
        /// Rejected path.
        path: String,
    },
}

impl fmt::Display for ToolInspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::NotOwned { path } = self;
        write!(f, "tool_file_not_owned:{path}")
    }
}

impl std::error::Error for ToolInspectError {}

/// Route a path to its owning stack id by file name, if any.
///
/// The Mise stack owns its tool and lock files; Cargo manifests, locks,
/// and `rust-toolchain.toml` route to the Rust stack while every other
/// symbol routes nowhere.
#[must_use]
pub fn stack_for_symbol(path: &str) -> Option<&'static str> {
    let name = file_name(path);
    if OWNED_SYMBOLS.contains(&name) {
        Some(crate::TOOL_ID)
    } else if FOREIGN_TOOL_FILES.contains(&name) {
        Some("rust")
    } else {
        None
    }
}

/// Whether `path` names a file this adapter may inspect.
#[must_use]
pub fn is_owned_mise_file(path: &str) -> bool {
    OWNED_SYMBOLS.contains(&file_name(path))
}

/// Whether `name` is a Mise-owned env symbol (`MISE_*`).
#[must_use]
pub fn is_mise_env_symbol(name: &str) -> bool {
    name.starts_with(MISE_ENV_PREFIX)
}

/// Final path segment of a repository-relative path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Inspect supplied Mise tool-file bytes (`None` when missing).
///
/// Read-only: content issues become findings with manual recommendations,
/// never errors and never writes.
///
/// # Errors
///
/// Returns [`ToolInspectError::NotOwned`] unless `path` names an owned
/// tool file.
pub fn inspect_mise_file(
    path: &str,
    content: Option<&str>,
) -> Result<MiseInspection, ToolInspectError> {
    if !is_owned_mise_file(path) {
        return Err(ToolInspectError::NotOwned {
            path: path.to_owned(),
        });
    }
    let Some(text) = content else {
        return Ok(MiseInspection::missing(path));
    };
    match parse_tools(text) {
        Ok(spec) => Ok(MiseInspection::present(path, spec)),
        Err(problem) => Ok(MiseInspection::invalid(path, &problem)),
    }
}

/// Reconciliation of the Cargo wrapper against the local tool pin.
///
/// A `wrappers.cargo.command = "mbx"` wrapper requires the `mbx` binary;
/// clean checkouts resolve it from the `[tools]` exact pin in this same
/// file, never from a global install (P07-9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MbxWrapperStatus {
    /// No MBX wrapper: no pin required.
    NoMbxWrapper,
    /// MBX wrapper with an exact local `mr-boxington` pin.
    Pinned {
        /// Exact pinned version selecting the `mbx` binary.
        version: String,
    },
    /// MBX wrapper without a local `mr-boxington` pin.
    MissingPin,
    /// Malformed wrapper or tool table, or a non-exact pin selector.
    Invalid {
        /// Machine-readable problem detail.
        problem: String,
    },
}

/// Reconcile one `mise.toml` text: wrapper-required MBX versus local pin.
///
/// Returns [`MbxWrapperStatus::NoMbxWrapper`] when no structural MBX
/// wrapper exists. A wrapper selecting MBX needs an exact
/// `mr-boxington` pin in `[tools]`; anything else fails closed.
#[must_use]
pub fn mbx_wrapper_pin_status(content: &str) -> MbxWrapperStatus {
    let wrapper = match parse_cargo_wrapper(content) {
        Ok(wrapper) => wrapper,
        Err(diagnostic) => {
            return MbxWrapperStatus::Invalid {
                problem: diagnostic.to_string(),
            };
        }
    };
    if !wrapper.is_some_and(|found| found.is_mbx()) {
        return MbxWrapperStatus::NoMbxWrapper;
    }
    let inspection = match inspect_mise_file(MISE_TOML_FILE, Some(content)) {
        Ok(inspection) => inspection,
        Err(error) => {
            return MbxWrapperStatus::Invalid {
                problem: error.to_string(),
            };
        }
    };
    let Some(spec) = inspection.spec else {
        let problem = inspection
            .findings
            .first()
            .and_then(|finding| finding.observed.clone())
            .unwrap_or_else(|| "unreadable_tools".to_owned());
        return MbxWrapperStatus::Invalid { problem };
    };
    let tool = PinnedTool::MrBoxington.tool_name();
    let Some(selector) = spec.tools.get(tool) else {
        return MbxWrapperStatus::MissingPin;
    };
    if validate_exact_version(tool, selector).is_ok() {
        MbxWrapperStatus::Pinned {
            version: selector.clone(),
        }
    } else {
        MbxWrapperStatus::Invalid {
            problem: format!("inexact_pin:{tool}:{selector}"),
        }
    }
}

/// Tool versions recorded in `mise.lock` bytes (TOOL-2.7).
///
/// Comparison input for lock-vs-toml conflict checks: malformed content
/// yields an empty map while [`inspect_mise_file`] reports the finding.
#[must_use]
pub fn lock_tool_versions(content: &str) -> BTreeMap<String, String> {
    match inspect_mise_file(MISE_LOCK_FILE, Some(content)) {
        Ok(inspection) => inspection.spec.map(|spec| spec.tools).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    }
}

/// Read tool choices from the structural TOML document.
///
/// Mise permits scalar versions and tool tables. Tool-table metadata does
/// not set a version. Malformed input or unsupported choices return an error.
fn parse_tools(content: &str) -> Result<MiseSpec, String> {
    let doc = parse_toml(content)
        .map_err(|failed| format!("line {}: {}", failed.line, failed.problem))?;
    if let Some((path, line)) = doc
        .array_sections
        .iter()
        .find(|(path, _)| path.first().map(String::as_str) == Some("tools"))
    {
        return Err(line_problem(
            *line,
            &format!("tool_array_table:{}", path.join(".")),
        ));
    }
    let mut tools = BTreeMap::new();
    for assignment in &doc.assignments {
        if assignment.path.first().map(String::as_str) != Some("tools") {
            continue;
        }
        let Some(tool) = assignment.path.get(1) else {
            continue;
        };
        match assignment.path.as_slice() {
            [_, _] => {
                let TomlValue::Str(version) = &assignment.value else {
                    return Err(line_problem(assignment.line, "tool_selector_not_string"));
                };
                tools.insert(tool.clone(), version.clone());
            }
            [_, _, field, rest @ ..] if field == "version" => {
                if !rest.is_empty() {
                    return Err(line_problem(assignment.line, "tool_version_not_scalar"));
                }
                let TomlValue::Str(version) = &assignment.value else {
                    return Err(line_problem(assignment.line, "tool_version_not_string"));
                };
                if tools.insert(tool.clone(), version.clone()).is_some() {
                    return Err(line_problem(assignment.line, "duplicate_tool_version"));
                }
            }
            [_, _, field, rest @ ..] if matches!(field.as_str(), "path" | "prefix" | "ref") => {
                if !rest.is_empty() {
                    return Err(line_problem(assignment.line, "tool_selector_not_scalar"));
                }
                return Err(line_problem(
                    assignment.line,
                    &format!("unsupported_tool_selector:{tool}:{field}"),
                ));
            }
            // Other tool-table fields hold metadata, not tool choices.
            [_, _, ..] => {}
            _ => {
                return Err(line_problem(assignment.line, "invalid_tool_path"));
            }
        }
    }
    Ok(MiseSpec { tools })
}

/// Render a source line and stable parser problem.
fn line_problem(line: u32, problem: &str) -> String {
    format!("line {line}: {problem}")
}
