// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{ApiRule, GitHub, Repo, parse};

use super::all_pages;

#[derive(Deserialize)]
struct CheckRun {
    name: String,
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
    pr_only_checks: &[String],
) -> Result<BTreeSet<String>, CiError> {
    if pr_only_checks.is_empty() {
        return Ok(BTreeSet::new());
    }
    let runs: Vec<CheckRun> = all_pages(github, "check runs", |page| {
        format!("/repos/{repo}/commits/{sha}/check-runs?filter=all&per_page=100&page={page}")
    })?;
    Ok(runs
        .into_iter()
        .map(|run| run.name.to_lowercase())
        .collect())
}

pub(super) fn require_pull_request(
    github: &dyn GitHub,
    repo: &Repo,
    branch: &str,
    rules: &[ApiRule],
) -> Result<(), CiError> {
    if rules.is_empty() {
        return Err(CiError::new(format!(
            "branch {branch} has no pull_request rule, so its required checks cannot be exempted as PR-only"
        )));
    }
    let mut verified = false;
    for rule in rules {
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
        let detail: Ruleset = parse(
            &github.get(&format!("/repos/{repo}/rulesets/{id}"))?,
            "pull_request ruleset",
        )?;
        verified |= detail.id == id
            && detail.source == source
            && detail.source_type == source_type
            && detail.target == "branch"
            && detail.enforcement == "active"
            && detail.bypass_actors.is_some_and(|actors| actors.is_empty())
            && detail.rules.iter().any(|rule| rule.kind == "pull_request");
    }
    if verified {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "branch {branch} has no verified non-bypassable pull_request ruleset; PR-only exemptions require matching ruleset identity and source, active branch enforcement, a pull_request rule and explicitly empty bypass_actors; verify the token can read bypass_actors, which GitHub hides without ruleset write access"
        )))
    }
}
