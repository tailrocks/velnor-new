// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{ApiRequiredCheck, ApiRule, GitHub, Repo, parse};

use super::all_pages;

#[derive(Deserialize)]
struct CheckRun {
    name: String,
    check_suite: Option<CheckSuiteId>,
}

#[derive(Deserialize)]
struct CheckSuiteId {
    id: u64,
}

#[derive(Deserialize)]
struct CheckSuite {
    id: u64,
    status: String,
    app: CheckApp,
}

#[derive(Deserialize)]
struct CheckApp {
    id: i64,
}

#[derive(Deserialize)]
struct Ruleset {
    id: u64,
    source: String,
    source_type: String,
    target: String,
    enforcement: String,
    bypass_actors: Option<Vec<serde_json::Value>>,
    rules: Vec<ApiRule>,
}

pub(super) fn check_names(
    github: &dyn GitHub,
    repo: &Repo,
    sha: &str,
    exempt_sources: &[ApiRequiredCheck],
) -> Result<BTreeSet<String>, CiError> {
    if exempt_sources.is_empty() {
        return Ok(BTreeSet::new());
    }
    let aggregate: Vec<CheckRun> = all_pages(github, "aggregate check runs", |page| {
        format!("/repos/{repo}/commits/{sha}/check-runs?filter=all&per_page=100&page={page}")
    })?;
    let mut covered = BTreeSet::new();
    let mut names = BTreeSet::new();
    for run in aggregate {
        let suite = run
            .check_suite
            .ok_or_else(|| CiError::new("aggregate check run has no check_suite identity"))?;
        covered.insert(suite.id);
        names.insert(run.name.to_lowercase());
    }
    let suites: Vec<CheckSuite> = all_pages(github, "check suites", |page| {
        format!("/repos/{repo}/commits/{sha}/check-suites?per_page=100&page={page}")
    })?;
    for suite in suites {
        if suite.status != "completed"
            && exempt_sources.iter().any(|check| {
                check
                    .integration_id
                    .is_none_or(|app| app == -1 || app == suite.app.id)
            })
        {
            return Err(CiError::new(format!(
                "unfinished check suite {} from App {} cannot prove a PR-only check absent",
                suite.id, suite.app.id
            )));
        }
        if covered.contains(&suite.id) {
            continue;
        }
        let runs: Vec<CheckRun> = all_pages(github, "check runs", |page| {
            format!(
                "/repos/{repo}/check-suites/{}/check-runs?filter=all&per_page=100&page={page}",
                suite.id
            )
        })?;
        names.extend(runs.into_iter().map(|run| run.name.to_lowercase()));
    }
    Ok(names)
}

pub(super) fn require_enforcement(
    github: &dyn GitHub,
    repo: &Repo,
    branch: &str,
    rules: &[ApiRule],
    exemptions: &[String],
    checks: &[ApiRequiredCheck],
) -> Result<(), CiError> {
    if !rules.iter().any(|rule| rule.kind == "pull_request") {
        return Err(CiError::new(format!(
            "branch {branch} has no pull_request rule, so its required checks cannot be exempted as PR-only"
        )));
    }
    let mut details = BTreeMap::new();
    let mut pull_request = false;
    let mut enforced = Vec::new();
    for rule in rules {
        if !matches!(
            rule.kind.as_str(),
            "pull_request" | "required_status_checks"
        ) {
            continue;
        }
        let (Some(id), Some(source), Some(source_type)) = (
            rule.ruleset_id,
            rule.ruleset_source.as_deref(),
            rule.ruleset_source_type.as_deref(),
        ) else {
            continue;
        };
        if source.is_empty() || !matches!(source_type, "Repository" | "Organization" | "Enterprise")
        {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = details.entry(id) {
            entry.insert(parse::<Ruleset>(
                &github.get(&format!("/repos/{repo}/rulesets/{id}"))?,
                "enforcement ruleset",
            )?);
        }
        let detail = &details[&id];
        let verified = detail.id == id
            && detail.source == source
            && detail.source_type == source_type
            && detail.target == "branch"
            && detail.enforcement == "active"
            && detail.bypass_actors.as_ref().is_some_and(Vec::is_empty);
        if !verified {
            continue;
        }
        pull_request |= rule.kind == "pull_request"
            && detail.rules.iter().any(|rule| rule.kind == "pull_request");
        if rule.kind == "required_status_checks" {
            for check in listed_checks(rule) {
                if detail
                    .rules
                    .iter()
                    .filter(|rule| rule.kind == "required_status_checks")
                    .flat_map(listed_checks)
                    .any(|actual| same_binding(check, actual))
                {
                    enforced.push(check);
                }
            }
        }
    }
    if !pull_request {
        return Err(CiError::new(format!(
            "branch {branch} has no verified non-bypassable pull_request ruleset; PR-only exemptions require matching ruleset identity and source, active branch enforcement, a pull_request rule and explicitly empty bypass_actors; verify the token can read bypass_actors, which GitHub hides without ruleset write access"
        )));
    }
    require_checks(branch, exemptions, checks, &enforced)
}

fn require_checks(
    branch: &str,
    exemptions: &[String],
    checks: &[ApiRequiredCheck],
    enforced: &[&ApiRequiredCheck],
) -> Result<(), CiError> {
    for name in exemptions {
        let obligations: Vec<_> = checks
            .iter()
            .filter(|check| &check.context == name)
            .collect();
        if obligations.is_empty()
            || obligations.iter().any(|check| {
                !enforced.iter().any(|safe| {
                    safe.context == check.context
                        && (check.integration_id.is_none_or(|app| app == -1)
                            || same_binding(check, safe))
                })
            })
        {
            return Err(CiError::new(format!(
                "branch {branch} has no verified non-bypassable required_status_checks ruleset for PR-only check {name}"
            )));
        }
    }
    Ok(())
}

fn listed_checks(rule: &ApiRule) -> impl Iterator<Item = &ApiRequiredCheck> {
    rule.parameters
        .as_ref()
        .and_then(|parameters| parameters.required_status_checks.as_ref())
        .into_iter()
        .flatten()
}

fn same_binding(left: &ApiRequiredCheck, right: &ApiRequiredCheck) -> bool {
    left.context == right.context
        && left.integration_id.filter(|id| *id != -1) == right.integration_id.filter(|id| *id != -1)
}
