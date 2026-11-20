// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::escape::escape;
use crate::github::{ApiRule, GitHub, Repo, parse, rules_path};

const RULES_PAGE: usize = 100;
const MAX_PAGES: u32 = 5;
const QUEUE_RULE: &str = "merge_queue";

#[derive(Debug)]
pub struct Context {
    pub event: String,
    pub repository: String,
    pub base: String,
}

fn warning(message: &str) -> String {
    format!(
        "::warning title=Label mode without a merge queue::{}",
        escape(message, false)
    )
}

fn unverified(base: &str) -> String {
    format!(
        "pr_ci = \"label\" is safe only with a merge queue: an unlabeled pull request runs only the quick checks, so the merge_group run is the only run of the full suite. The active rulesets of {base} require no merge queue, so unverified code can merge. Add a merge queue rule that requires the Required check to {base}. If {base} uses the merge queue setting of classic branch protection, the API does not show it and this warning can be ignored."
    )
}

fn unreadable(base: &str, reason: &str) -> String {
    format!(
        "Could not check whether {base} requires a merge queue ({reason}). pr_ci = \"label\" is safe only with a merge queue; check the repository settings by hand."
    )
}

fn has_queue(github: &impl GitHub, repo: &Repo, base: &str) -> Result<bool, String> {
    for page in 1..=MAX_PAGES {
        let body = github
            .get(&rules_path(repo, base, page))
            .map_err(|error| error.to_string())?;
        let rules: Vec<ApiRule> =
            parse(&body, "branch rules").map_err(|error| error.to_string())?;
        if rules.iter().any(|rule| rule.kind == QUEUE_RULE) {
            return Ok(true);
        }
        if rules.len() < RULES_PAGE {
            break;
        }
    }
    Ok(false)
}

/// The `::warning::` line to print when this pull request run targets a branch
/// that no active ruleset puts behind a merge queue, a warning that says the
/// lookup failed when the rules cannot be read, and nothing otherwise. It
/// never fails: label mode is only safe with a queue, but a lookup is not a gate.
#[must_use]
pub fn queue_warning(github: &impl GitHub, context: &Context) -> Option<String> {
    if context.event != "pull_request" {
        return None;
    }
    let base = context.base.as_str();
    if base.is_empty() {
        return Some(warning(&unreadable(
            "the base branch",
            "GITHUB_BASE_REF is empty",
        )));
    }
    let repo = match Repo::parse(&context.repository) {
        Ok(repo) => repo,
        Err(error) => return Some(warning(&unreadable(base, &error.to_string()))),
    };
    match has_queue(github, &repo, base) {
        Ok(true) => None,
        Ok(false) => Some(warning(&unverified(base))),
        Err(reason) => Some(warning(&unreadable(base, &reason))),
    }
}
