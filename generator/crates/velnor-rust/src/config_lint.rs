// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;
use velnor_model::{JobId, LintJob, LintKind, RepoPath, RepoView, RustCiConfig, ToolPins};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};
use crate::mise::tool_pins;

pub(crate) const JACTIONLINT: &str = "jactionlint";
pub(crate) const SHELLCHECK: &str = "shellcheck";
const ALINT: &str = "alint";
const MISE_TASK: &str = "mise-task";
const PREFIX: &str = "lint-";
const DEFAULT_TIMEOUT_MINUTES: u16 = 10;
const TIMEOUT_RANGE: std::ops::RangeInclusive<u16> = 1..=360;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLint {
    kind: String,
    id: Option<String>,
    name: Option<String>,
    timeout_minutes: Option<u16>,
    config: Option<String>,
    probe: Option<String>,
    task: Option<String>,
    tools: Option<Vec<String>>,
}

fn word(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:@".contains(c))
}

fn path(key: &'static str, value: Option<String>) -> Result<Option<RepoPath>, Reason> {
    value
        .map(|text| {
            let invalid = |value| Reason::InvalidValue { key, value };
            if !word(&text) {
                return Err(invalid(text));
            }
            RepoPath::new(text.clone()).map_err(|_| invalid(text))
        })
        .transpose()
}

fn foreign(kind: &str, key: &'static str, present: bool) -> Result<(), Reason> {
    if present {
        Err(Reason::LintKeyForeign {
            kind: kind.to_owned(),
            key,
        })
    } else {
        Ok(())
    }
}

fn identifier(raw_id: Option<String>, fallback: &str) -> Result<JobId, Reason> {
    let tail = raw_id.unwrap_or_else(|| {
        fallback
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    });
    JobId::new(format!("{PREFIX}{tail}")).map_err(|_| Reason::InvalidValue {
        key: "lint.id",
        value: tail,
    })
}

fn title(raw_name: Option<String>, fallback: String) -> Result<String, Reason> {
    let name = raw_name.unwrap_or(fallback);
    if name.trim().is_empty() || name.contains("${{") || name.chars().any(char::is_control) {
        return Err(Reason::InvalidValue {
            key: "lint.name",
            value: name,
        });
    }
    Ok(name)
}

fn lint(raw: RawLint) -> Result<LintJob, Reason> {
    let kind = raw.kind.as_str();
    let config = path("lint.config", raw.config.clone())?;
    let probe = path("lint.probe", raw.probe.clone())?;
    let (kind, id, name) = match kind {
        JACTIONLINT => {
            foreign(kind, "probe", probe.is_some())?;
            foreign(kind, "task", raw.task.is_some())?;
            foreign(kind, "tools", raw.tools.is_some())?;
            (
                LintKind::Jactionlint { config },
                JACTIONLINT.to_owned(),
                "Jactionlint".to_owned(),
            )
        }
        ALINT => {
            foreign(kind, "task", raw.task.is_some())?;
            foreign(kind, "tools", raw.tools.is_some())?;
            let config = config.ok_or(Reason::MissingKey("lint.config"))?;
            (
                LintKind::Alint { config, probe },
                ALINT.to_owned(),
                "Alint".to_owned(),
            )
        }
        MISE_TASK => {
            foreign(kind, "config", config.is_some())?;
            foreign(kind, "probe", probe.is_some())?;
            let task = raw.task.clone().ok_or(Reason::MissingKey("lint.task"))?;
            if !word(&task) {
                return Err(Reason::InvalidValue {
                    key: "lint.task",
                    value: task,
                });
            }
            let tools = raw.tools.clone().unwrap_or_default();
            if let Some(tool) = tools.iter().find(|tool| !word(tool)) {
                return Err(Reason::InvalidValue {
                    key: "lint.tools",
                    value: tool.clone(),
                });
            }
            let id = task.clone();
            let name = format!("Mise task {task}");
            (LintKind::MiseTask { task, tools }, id, name)
        }
        other => {
            return Err(Reason::InvalidValue {
                key: "lint.kind",
                value: other.to_owned(),
            });
        }
    };
    let timeout_minutes = raw.timeout_minutes.unwrap_or(DEFAULT_TIMEOUT_MINUTES);
    if !TIMEOUT_RANGE.contains(&timeout_minutes) {
        return Err(Reason::OutOfRange {
            key: "lint.timeout_minutes",
            value: timeout_minutes,
            min: *TIMEOUT_RANGE.start(),
            max: *TIMEOUT_RANGE.end(),
        });
    }
    Ok(LintJob {
        id: identifier(raw.id, &id)?,
        name: title(raw.name, name)?,
        timeout_minutes,
        kind,
    })
}

pub(crate) fn parse(raw: Vec<RawLint>) -> Result<Vec<LintJob>, Reason> {
    let jobs = raw.into_iter().map(lint).collect::<Result<Vec<_>, _>>()?;
    let mut seen = BTreeSet::new();
    match jobs.iter().find(|job| !seen.insert(job.id.clone())) {
        Some(job) => Err(Reason::LintDuplicate(job.id.to_string())),
        None => Ok(jobs),
    }
}

pub(crate) fn tool_key<'a>(tools: &'a ToolPins, name: &str) -> Option<&'a str> {
    tools
        .tools
        .keys()
        .find(|key| key.rsplit(['/', ':']).next() == Some(name))
        .map(String::as_str)
}

pub(crate) fn pinned_tools(
    view: &RepoView,
    config: &RustCiConfig,
) -> Result<ToolPins, DetectError> {
    let tools = tool_pins(&view.files)?;
    check(config, &tools)?;
    Ok(tools)
}

fn check(config: &RustCiConfig, tools: &ToolPins) -> Result<(), DetectError> {
    for job in &config.lint {
        let names: Vec<&str> = match &job.kind {
            LintKind::Jactionlint { .. } => vec![JACTIONLINT],
            LintKind::Alint { .. } => Vec::new(),
            LintKind::MiseTask { tools, .. } => tools.iter().map(String::as_str).collect(),
        };
        if let Some(name) = names
            .into_iter()
            .find(|name| tool_key(tools, name).is_none())
        {
            return Err(DetectError::invalid(
                &CONFIG_PATH,
                Reason::LintToolUnpinned(name.to_owned()),
            ));
        }
    }
    Ok(())
}
