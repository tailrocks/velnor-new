//! Read-only `rust-toolchain.toml` inspection owned by the Rust adapter.
//!
//! Pure over bytes the orchestrator supplies: this module never reads,
//! writes, or stats the filesystem. Findings always name the supplying
//! file; missing or malformed files yield findings plus manual
//! recommendations, never repairs (tooling-input §1-§3).

use std::fmt;

/// Tool-file name owned by the Rust adapter.
pub const RUST_TOOLCHAIN_FILE: &str = "rust-toolchain.toml";

/// File symbols routed to the Rust stack (quality §1).
pub const OWNED_SYMBOLS: &[&str] = &["Cargo.toml", "Cargo.lock", RUST_TOOLCHAIN_FILE];

/// Tool files owned by sibling adapters; recognized but never inspected here.
pub const FOREIGN_TOOL_FILES: &[&str] = &["mise.toml", "mise.lock"];

/// Selection-broadening class one changed path triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionBroadening {
    /// Lockfile change: every unit is affected.
    Lockfile,
    /// Root configuration change: every unit is affected.
    RootConfig,
}

/// Broadening class for one changed path, if it invalidates selection.
///
/// Only the root lockfile and root Cargo configuration broaden; nested
/// paths resolve through ownership and edges instead.
#[must_use]
pub fn selection_broadening(path: &str) -> Option<SelectionBroadening> {
    if path == "Cargo.lock" {
        return Some(SelectionBroadening::Lockfile);
    }
    if path == "Cargo.toml" || path == ".cargo/config.toml" || path == ".cargo/config" {
        return Some(SelectionBroadening::RootConfig);
    }
    None
}

/// True for toolfiles the rust adapter recognizes (own plus foreign).
///
/// `.mise.toml` (hidden variant) stays orchestrator-global and is not
/// covered here.
#[must_use]
pub fn is_known_toolfile(path: &str) -> bool {
    path == RUST_TOOLCHAIN_FILE || FOREIGN_TOOL_FILES.contains(&path)
}

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

/// Extracted `[toolchain]` selection (reported values only, never pins).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolchainSpec {
    /// Selected channel (`stable`, `beta`, `nightly`, or `X.Y.Z`), if declared.
    pub channel: Option<String>,
    /// Requested components, sorted.
    pub components: Vec<String>,
    /// Requested targets, sorted.
    pub targets: Vec<String>,
}

/// Outcome of one owned tool-file inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainInspection {
    /// Inspected repository-relative file.
    pub file: String,
    /// Extracted selection (`None` when missing or malformed).
    pub spec: Option<ToolchainSpec>,
    /// Findings plus manual recommendations; empty when valid and pinned.
    pub findings: Vec<ToolFinding>,
}

impl ToolchainInspection {
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
                    "add a user-authored [toolchain] channel pin in a new \
                     {path}; Velnor never creates it"
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
                    "fix {problem} manually in {path}; generation continues \
                     with Velnor's pinned Rust"
                ),
            }],
        }
    }

    /// Inspection for a readable tool file, with a pin finding when unpinned.
    fn present(path: &str, spec: ToolchainSpec) -> Self {
        let findings = if spec.channel.as_deref().is_none_or(str::is_empty) {
            vec![ToolFinding {
                code: MISSING_RECOMMENDED_INPUT.to_owned(),
                file: path.to_owned(),
                observed: Some("no [toolchain] channel".to_owned()),
                recommendation: format!(
                    "add channel = \"<pinned stable>\" under [toolchain] in \
                     {path}; Velnor never edits it"
                ),
            }]
        } else {
            Vec::new()
        };
        Self {
            file: path.to_owned(),
            spec: Some(spec),
            findings,
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
/// The Rust stack owns Cargo manifests and locks plus
/// `rust-toolchain.toml`; Mise tool files route to the Mise adapter and
/// every other symbol routes nowhere.
#[must_use]
pub fn stack_for_symbol(path: &str) -> Option<&'static str> {
    let name = file_name(path);
    if OWNED_SYMBOLS.contains(&name) {
        Some(crate::STACK_ID)
    } else if FOREIGN_TOOL_FILES.contains(&name) {
        Some("mise")
    } else {
        None
    }
}

/// Whether `path` names a file this adapter may inspect.
#[must_use]
pub fn is_owned_tool_file(path: &str) -> bool {
    file_name(path) == RUST_TOOLCHAIN_FILE
}

/// Final path segment of a repository-relative path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Inspect supplied `rust-toolchain.toml` bytes (`None` when missing).
///
/// Read-only: content issues become findings with manual recommendations,
/// never errors and never writes.
///
/// # Errors
///
/// Returns [`ToolInspectError::NotOwned`] unless `path` names the owned
/// tool file.
pub fn inspect_toolchain_file(
    path: &str,
    content: Option<&str>,
) -> Result<ToolchainInspection, ToolInspectError> {
    if !is_owned_tool_file(path) {
        return Err(ToolInspectError::NotOwned {
            path: path.to_owned(),
        });
    }
    let Some(text) = content else {
        return Ok(ToolchainInspection::missing(path));
    };
    match parse_toolchain(text) {
        Ok(spec) => Ok(ToolchainInspection::present(path, spec)),
        Err(problem) => Ok(ToolchainInspection::invalid(path, &problem)),
    }
}

/// Parse `[toolchain]` channel/components/targets; unknown keys ignored.
fn parse_toolchain(content: &str) -> Result<ToolchainSpec, String> {
    let mut spec = ToolchainSpec::default();
    let mut in_toolchain = false;
    for (index, line) in content.lines().enumerate() {
        let code = crate::scan::strip_comment(line).trim();
        if code.is_empty() {
            continue;
        }
        if let Some(section) = parse_section(code) {
            in_toolchain = section == "toolchain";
            continue;
        }
        if code.starts_with('[') {
            return Err(line_problem(index, "unterminated_section"));
        }
        if !in_toolchain {
            continue;
        }
        let (key, value) = split_setting(code, index)?;
        match key {
            "channel" => spec.channel = Some(parse_string(value, index)?),
            "components" => spec.components = parse_string_list(value, index)?,
            "targets" => spec.targets = parse_string_list(value, index)?,
            _ => {}
        }
    }
    spec.components.sort();
    spec.components.dedup();
    spec.targets.sort();
    spec.targets.dedup();
    Ok(spec)
}

/// Section name of a `[section]` header, if well-formed.
fn parse_section(code: &str) -> Option<&str> {
    let section = code.strip_prefix('[')?.strip_suffix(']')?;
    Some(section.trim())
}

/// Split one `key = value` setting inside `[toolchain]`.
fn split_setting(code: &str, index: usize) -> Result<(&str, &str), String> {
    let (key, value) = code
        .split_once('=')
        .ok_or_else(|| line_problem(index, "expected_key_equals_value"))?;
    let key = key.trim();
    if key.is_empty() || value.trim().is_empty() {
        return Err(line_problem(index, "expected_key_equals_value"));
    }
    Ok((key, value.trim()))
}

/// Parse one quoted TOML basic or literal string.
fn parse_string(value: &str, index: usize) -> Result<String, String> {
    let quote = value.chars().next().unwrap_or('\0');
    if (quote != '"' && quote != '\'') || value.len() < 2 || !value.ends_with(quote) {
        return Err(line_problem(index, "unterminated_string"));
    }
    Ok(value[1..value.len() - 1].to_owned())
}

/// Parse a `["a", "b"]` string list (trailing commas tolerated).
fn parse_string_list(value: &str, index: usize) -> Result<Vec<String>, String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| line_problem(index, "unterminated_list"))?;
    let mut items = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        items.push(parse_string(item, index)?);
    }
    Ok(items)
}

/// `line N: problem` detail with one-based line numbers.
fn line_problem(index: usize, problem: &str) -> String {
    format!("line {}: {problem}", index.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadening_pins_root_lock_and_config_only() {
        assert_eq!(
            selection_broadening("Cargo.lock"),
            Some(SelectionBroadening::Lockfile)
        );
        for path in ["Cargo.toml", ".cargo/config.toml", ".cargo/config"] {
            assert_eq!(
                selection_broadening(path),
                Some(SelectionBroadening::RootConfig),
                "{path:?}"
            );
        }
        for path in [
            "crates/a/Cargo.lock",
            "crates/a/Cargo.toml",
            "src/lib.rs",
            ".mise.toml",
            "mise.toml",
            "rust-toolchain.toml",
        ] {
            assert_eq!(selection_broadening(path), None, "{path:?}");
        }
    }

    #[test]
    fn known_toolfiles_cover_own_and_foreign() {
        for path in ["rust-toolchain.toml", "mise.toml", "mise.lock"] {
            assert!(is_known_toolfile(path), "{path:?}");
        }
        for path in [".mise.toml", "Cargo.toml", "Cargo.lock", "src/lib.rs"] {
            assert!(!is_known_toolfile(path), "{path:?}");
        }
    }
}
