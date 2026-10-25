//! Fail-closed presence preflight for pinned tools.
//!
//! Mise prints `WARN missing:` on stderr and still executes the payload
//! when a selected tool is not installed, so a nonzero-exit classifier
//! can never see the gap. [`ToolPresence`] queries installed versions
//! first (`mise <globals> ls --json <tool>` under the same isolation
//! overlay as `exec`) and fails closed naming the unsatisfied spec
//! before any payload can run.

use serde::Deserialize;
use serde::Serialize;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use velnor_actions_contract::parse_strict_json;

const MAX_OBSERVATION_STRING_BYTES: usize = 48;
const MAX_OBSERVATION_OTHER_ROWS: usize = 8;
const MAX_OBSERVATION_JSON_BYTES: usize = 4096;

/// Require installed presence for pinned tools before any payload spawn.
///
/// The query is read-only: `mise ls` lists versions, installs nothing.
/// Scoped to plain catalog tools addressed by their registry name, with
/// no backend aliasing: `mise ls --json <tool>` and
/// `mise exec <tool>@<exact>` read the same installs directory under the
/// same isolation overlay, so the list is a faithful proxy for what exec
/// selects. Backend-qualified `aqua:` selectors are deliberately NOT
/// covered here: their aliasing made `mise ls` miss versions that `exec`
/// resolves (proven on CI), so their presence is never gated on `ls`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPresence {
    /// Tools that must already be installed.
    tools: Vec<PinnedTool>,
}

impl ToolPresence {
    /// Require installed presence for the given pinned tools.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] for zero tools.
    pub fn new(tools: Vec<PinnedTool>) -> Result<Self, MiseError> {
        if tools.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        Ok(Self { tools })
    }

    /// Installed-version query for one pinned tool.
    #[must_use]
    pub fn command(&self, tool: PinnedTool) -> IsolatedCommand {
        IsolatedCommand::mise_ls_json(tool.tool_name())
    }

    /// Fail unless every pinned tool is installed; never spawns a payload.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::MissingToolPin`] naming the first unsatisfied
    /// spec and the versions actually installed, and propagates spawn,
    /// nonzero-exit, or unparsable-output failures of the read-only
    /// `mise ls` query itself (absence of evidence fails closed too).
    pub fn run(&self, catalog: &ToolCatalog) -> Result<(), MiseError> {
        self.run_with_observation(catalog)
            .map_err(|failure| *failure.error)
    }

    /// Fail unless every pinned tool is installed, retaining a bounded view of
    /// the failed query rows without rerunning Mise.
    ///
    /// # Errors
    ///
    /// Returns the original query, parse, or missing-pin failure. A missing-pin
    /// failure includes an observation only when the same query returned valid
    /// JSON.
    pub fn run_with_observation(&self, catalog: &ToolCatalog) -> Result<(), ToolPresenceFailure> {
        for tool in &self.tools {
            let output = self
                .command(*tool)
                .run()
                .map_err(ToolPresenceFailure::without_observation)?;
            output
                .require_success("mise")
                .map_err(ToolPresenceFailure::without_observation)?;
            let json = output
                .stdout_text("mise")
                .map_err(ToolPresenceFailure::without_observation)?;
            let version = catalog.version(*tool);
            let (installed, observation) = installed_versions_with_observation(&json, version)
                .map_err(ToolPresenceFailure::without_observation)?;
            if installed.iter().any(|seen| seen == version) {
                continue;
            }
            return Err(ToolPresenceFailure {
                error: Box::new(MiseError::MissingToolPin {
                    spec: catalog.tool_spec(*tool),
                    installed,
                }),
                observation: Some(Box::new(observation)),
            });
        }
        Ok(())
    }
}

/// One failed presence check, preserving the original error and optional
/// allowlisted observation from the same `mise ls` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPresenceFailure {
    /// The original command, parse, or missing-pin failure.
    pub error: Box<MiseError>,
    /// Bounded selected-row metadata when the query returned valid JSON.
    pub observation: Option<Box<ToolPresenceObservation>>,
}

impl ToolPresenceFailure {
    fn without_observation(error: MiseError) -> Self {
        Self {
            error: Box::new(error),
            observation: None,
        }
    }
}

/// Bounded, allowlisted rows from one failed `mise ls --json` query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolPresenceObservation {
    requested_version: String,
    requested_row: Option<ObservedToolRow>,
    other_rows: Vec<ObservedToolRow>,
    other_rows_truncated: bool,
}

impl ToolPresenceObservation {
    /// Compact single-line JSON, capped independently of Mise's raw response.
    #[must_use]
    pub fn to_json_line(&self) -> String {
        match serde_json::to_vec(self) {
            Ok(bytes) if bytes.len() <= MAX_OBSERVATION_JSON_BYTES => {
                String::from_utf8_lossy(&bytes).into_owned()
            }
            _ => "{\"observation\":\"unavailable_or_over_limit\"}".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ObservedToolRow {
    version: Option<String>,
    installed: Option<bool>,
    broken: Option<bool>,
    install_path: Option<String>,
    symlinked_to: Option<String>,
}

/// One `mise ls --json` entry.
#[derive(Deserialize)]
struct InstalledTool {
    /// Installed version selector.
    version: String,
    /// Whether the version is installed (present and complete).
    #[serde(default)]
    installed: bool,
}

/// Sorted installed versions reported by `mise ls --json <tool>`.
///
/// Any shape drift fails closed with an empty list, which [`MiseError::MissingToolPin`]
/// reports instead of silently passing.
#[cfg(test)]
fn installed_versions(json: &str) -> Result<Vec<String>, MiseError> {
    parse_installed_entries(json).map(|(_, entries)| installed_version_names(entries))
}

fn installed_versions_with_observation(
    json: &str,
    requested_version: &str,
) -> Result<(Vec<String>, ToolPresenceObservation), MiseError> {
    let (value, entries) = parse_installed_entries(json)?;
    let observation = observe_rows(&value, requested_version);
    Ok((installed_version_names(entries), observation))
}

fn parse_installed_entries(
    json: &str,
) -> Result<(serde_json::Value, Vec<InstalledTool>), MiseError> {
    let value = parse_strict_json(json).map_err(|err| MiseError::Contract {
        problem: format!("mise_ls_unparsable:{err}"),
    })?;
    let entries: Vec<InstalledTool> =
        serde_json::from_value(value.clone()).map_err(|err| MiseError::Contract {
            problem: format!("mise_ls_unparsable:{err}"),
        })?;
    Ok((value, entries))
}

fn installed_version_names(entries: Vec<InstalledTool>) -> Vec<String> {
    let mut versions: Vec<String> = entries
        .into_iter()
        .filter(|entry| entry.installed)
        .map(|entry| entry.version)
        .collect();
    versions.sort();
    versions
}

fn observe_rows(value: &serde_json::Value, requested_version: &str) -> ToolPresenceObservation {
    let rows = value.as_array().map(Vec::as_slice).unwrap_or_default();
    let requested_row = rows
        .iter()
        .find(|row| {
            row.get("version").and_then(serde_json::Value::as_str) == Some(requested_version)
        })
        .map(observe_row);
    let mut other_rows = Vec::new();
    let mut other_rows_truncated = false;
    for row in rows {
        if row.get("version").and_then(serde_json::Value::as_str) == Some(requested_version) {
            continue;
        }
        if other_rows.len() == MAX_OBSERVATION_OTHER_ROWS {
            other_rows_truncated = true;
            break;
        }
        other_rows.push(observe_row(row));
    }
    ToolPresenceObservation {
        requested_version: bounded_string(requested_version),
        requested_row,
        other_rows,
        other_rows_truncated,
    }
}

fn observe_row(value: &serde_json::Value) -> ObservedToolRow {
    ObservedToolRow {
        version: value
            .get("version")
            .and_then(serde_json::Value::as_str)
            .map(bounded_string),
        installed: value.get("installed").and_then(serde_json::Value::as_bool),
        broken: value.get("broken").and_then(serde_json::Value::as_bool),
        install_path: value
            .get("install_path")
            .and_then(serde_json::Value::as_str)
            .map(bounded_string),
        symlinked_to: value
            .get("symlinked_to")
            .and_then(serde_json::Value::as_str)
            .map(bounded_string),
    }
}

fn bounded_string(value: &str) -> String {
    let mut bounded = String::new();
    for character in value.chars() {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        if bounded.len() + character.len_utf8() > MAX_OBSERVATION_STRING_BYTES {
            break;
        }
        bounded.push(character);
    }
    bounded
}

#[cfg(test)]
#[path = "requests_presence_tests.rs"]
mod tests;
