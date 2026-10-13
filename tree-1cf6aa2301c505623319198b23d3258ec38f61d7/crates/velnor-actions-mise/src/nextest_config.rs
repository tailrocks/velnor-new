//! Structural Cargo Nextest configuration inspection.
//!
//! Pure over bytes the orchestrator supplies: this module never reads the
//! filesystem. It structurally lists `[profile.*]` sections in
//! `.config/nextest.toml`; CI selects `[profile.ci]` when present and
//! Nextest's documented `default` profile otherwise. Malformed TOML is a
//! [`NextestDiagnostic`], never a guess.

use std::collections::BTreeMap;
use std::fmt;

use crate::toml_scan::parse_toml;

/// Repository-relative Nextest configuration path.
pub const NEXTEST_CONFIG_REL: &str = ".config/nextest.toml";

/// CI profile name selected when its section is present.
pub const CI_PROFILE_NAME: &str = "ci";

/// Nextest's documented default profile (used without `--profile`).
pub const DEFAULT_PROFILE_NAME: &str = "default";

/// Structurally resolved Nextest configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestConfig {
    /// Sorted profile names declared under `[profile.*]`.
    pub profiles: Vec<String>,
    /// One-based line declaring `[profile.ci]`, when present.
    pub ci_line: Option<u32>,
}

impl NextestConfig {
    /// Whether `[profile.ci]` is structurally present.
    #[must_use]
    pub fn has_ci_profile(&self) -> bool {
        self.ci_line.is_some()
    }

    /// Profile CI selects: `ci` when present, else the documented default.
    #[must_use]
    pub fn selected_profile(&self) -> &'static str {
        if self.has_ci_profile() {
            CI_PROFILE_NAME
        } else {
            DEFAULT_PROFILE_NAME
        }
    }
}

/// Strict Nextest-config inspection failure naming the offending line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestDiagnostic {
    /// One-based line of the failure.
    pub line: u32,
    /// Stable problem code.
    pub problem: String,
}

impl fmt::Display for NextestDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nextest_config_invalid:{}:{}", self.line, self.problem)
    }
}

impl std::error::Error for NextestDiagnostic {}

/// Resolve the Nextest profiles declared in config bytes.
///
/// # Errors
///
/// Returns [`NextestDiagnostic`] on malformed TOML.
pub fn parse_nextest_config(content: &str) -> Result<NextestConfig, NextestDiagnostic> {
    let doc = parse_toml(content).map_err(|failed| NextestDiagnostic {
        line: failed.line,
        problem: failed.problem,
    })?;
    let mut lines: BTreeMap<String, u32> = BTreeMap::new();
    for (path, line) in &doc.sections {
        record_profile(&mut lines, path, *line);
    }
    for item in &doc.assignments {
        if item.path.len() >= 3 {
            record_profile(&mut lines, &item.path[..2], item.line);
        }
    }
    let mut profiles: Vec<String> = lines.keys().cloned().collect();
    profiles.sort();
    Ok(NextestConfig {
        profiles,
        ci_line: lines.get(CI_PROFILE_NAME).copied(),
    })
}

/// Record the profile named by a `[profile.<name>]` path prefix.
fn record_profile(lines: &mut BTreeMap<String, u32>, path: &[String], line: u32) {
    if path.len() < 2 || path[0] != "profile" {
        return;
    }
    lines.entry(path[1].clone()).or_insert(line);
}
