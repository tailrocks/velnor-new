// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{LintJob, LintKind, RepoView, RunnerLabel, RustCiConfig, ToolPin, ToolPins};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};
use crate::lint_error::LintFault;
use crate::lint_tasks::{self, Catalog};
use crate::mise_lock::lock_platform;

pub(crate) const JACTIONLINT: &str = "jactionlint";
pub(crate) const SHELLCHECK: &str = "shellcheck";
pub(crate) const ALINT: &str = "alint";
pub(crate) const NODE: &str = "node";

pub(crate) enum Lookup<'a> {
    Missing,
    One(&'a str),
    Many(Vec<&'a str>),
}

fn basename(key: &str) -> &str {
    key.rsplit(['/', ':']).next().unwrap_or(key)
}

pub(crate) fn lookup<'a>(tools: &'a ToolPins, name: &str) -> Lookup<'a> {
    if let Some((key, _)) = tools.tools.get_key_value(name) {
        return Lookup::One(key);
    }
    let mut keys: Vec<&str> = tools
        .tools
        .keys()
        .map(String::as_str)
        .filter(|key| basename(key) == name)
        .collect();
    match keys.len() {
        0 => Lookup::Missing,
        1 => Lookup::One(keys.remove(0)),
        _ => Lookup::Many(keys),
    }
}

fn version_applies(pin: &ToolPin, version: &str, platform: Option<&str>) -> bool {
    let Some(platform) = platform else {
        return true;
    };
    let os = platform.split('-').next().unwrap_or(platform);
    pin.os_filters
        .get(version)
        .is_none_or(|names| names.iter().any(|name| name == os || name == platform))
}

pub(crate) fn applies(pin: &ToolPin, platform: Option<&str>) -> bool {
    pin.requested.is_empty()
        || pin
            .requested
            .iter()
            .any(|version| version_applies(pin, version, platform))
}

pub(crate) fn usable<'a>(tools: &'a ToolPins, runner: &RunnerLabel, name: &str) -> Option<&'a str> {
    match lookup(tools, name) {
        Lookup::One(key) => tools
            .tools
            .get(key)
            .filter(|pin| applies(pin, lock_platform(runner)))
            .map(|_| key),
        _ => None,
    }
}

fn numeric(version: &str) -> bool {
    version
        .trim_start_matches('v')
        .starts_with(|c: char| c.is_ascii_digit())
}

fn covers(locked: &str, requested: &str) -> bool {
    let locked = locked.trim_start_matches('v');
    let requested = requested.trim_start_matches('v');
    locked == requested
        || locked
            .strip_prefix(requested)
            .is_some_and(|rest| rest.starts_with(['.', '-', '+']))
}

fn stale_version<'a>(pin: &'a ToolPin, platform: Option<&str>) -> Option<&'a str> {
    pin.requested
        .iter()
        .filter(|version| numeric(version) && version_applies(pin, version, platform))
        .find(|version| !pin.locked.iter().any(|locked| covers(locked, version)))
        .map(String::as_str)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Need {
    Required,
    Optional,
}

fn verify(
    tools: &ToolPins,
    platform: Option<&str>,
    name: &str,
    need: Need,
) -> Result<(), LintFault> {
    let key = match lookup(tools, name) {
        Lookup::Missing if need == Need::Required => {
            return Err(LintFault::ToolUnpinned(name.to_owned()));
        }
        Lookup::Missing => return Ok(()),
        Lookup::Many(keys) => {
            return Err(LintFault::ToolAmbiguous {
                tool: name.to_owned(),
                keys: keys.into_iter().map(str::to_owned).collect(),
            });
        }
        Lookup::One(key) => key,
    };
    let pin = &tools.tools[key];
    if !applies(pin, platform) {
        return match need {
            Need::Required => Err(LintFault::ToolFiltered {
                tool: name.to_owned(),
                platform: platform.unwrap_or_default().to_owned(),
            }),
            Need::Optional => Ok(()),
        };
    }
    match tools
        .locked_file
        .then(|| stale_version(pin, platform))
        .flatten()
    {
        Some(version) => Err(LintFault::ToolLockStale {
            tool: name.to_owned(),
            version: version.to_owned(),
        }),
        None => Ok(()),
    }
}

fn runs_github_script(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        let line = line.strip_prefix("- ").unwrap_or(line).trim_start();
        line.strip_prefix("uses:").is_some_and(|action| {
            action
                .trim()
                .trim_matches(['"', '\''])
                .starts_with("actions/github-script@")
        })
    })
}

fn uses_github_script(view: &RepoView) -> bool {
    view.files
        .iter()
        .filter(|(path, _)| path.as_str().starts_with(".github/workflows/"))
        .any(|(_, bytes)| runs_github_script(&String::from_utf8_lossy(bytes)))
}

fn check_job(
    job: &LintJob,
    view: &RepoView,
    tools: &ToolPins,
    platform: Option<&str>,
    catalog: &Catalog,
) -> Result<(), LintFault> {
    let check = |name: &str, need| verify(tools, platform, name, need);
    match &job.kind {
        LintKind::Jactionlint { .. } => {
            check(JACTIONLINT, Need::Required)?;
            check(SHELLCHECK, Need::Optional)?;
            if uses_github_script(view) {
                check(NODE, Need::Required).map_err(|fault| match fault {
                    LintFault::ToolUnpinned(_) | LintFault::ToolFiltered { .. } => {
                        LintFault::NodeRequired
                    }
                    other => other,
                })
            } else {
                check(NODE, Need::Optional)
            }
        }
        LintKind::Alint { .. } => check(ALINT, Need::Required),
        LintKind::MiseTask { task, tools: named } => {
            for name in named {
                check(name, Need::Required)?;
            }
            lint_tasks::check(catalog, tools, task, named)
        }
    }
}

pub(crate) fn check(
    view: &RepoView,
    config: &RustCiConfig,
    tools: &ToolPins,
) -> Result<(), DetectError> {
    let platform = lock_platform(&config.runner);
    let catalog = if config
        .lint
        .iter()
        .any(|job| matches!(job.kind, LintKind::MiseTask { .. }))
    {
        lint_tasks::catalog(view, tools)
            .map_err(|reason| DetectError::invalid(&CONFIG_PATH, reason))?
    } else {
        Catalog::default()
    };
    config.lint.iter().try_for_each(|job| {
        check_job(job, view, tools, platform, &catalog)
            .map_err(|fault| DetectError::invalid(&CONFIG_PATH, Reason::Lint(fault)))
    })
}
