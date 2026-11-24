// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::error::CiError;
use crate::github::{
    ApiBranch, ApiRequiredCheck, ApiRule, GitHub, Repo, branch_path, parse, rules_path,
};

use super::exemptions;

const ACTIONS_APP_ID: i64 = 15368;
const ANY_APP: i64 = -1;
const RULES_PAGE: usize = 100;

pub(super) struct RequiredChecks {
    pub(super) names: BTreeSet<String>,
    pub(super) exempt_sources: Vec<ApiRequiredCheck>,
}

pub(super) fn required_checks(
    github: &dyn GitHub,
    repo: &Repo,
    branch: &str,
    pr_only_checks: &[String],
) -> Result<RequiredChecks, CiError> {
    let mut checks = BTreeSet::new();
    let mut app_checks = Vec::new();
    let mut applicable = Vec::new();
    for page in 1.. {
        let rules: Vec<ApiRule> = parse(
            &github.get(&rules_path(repo, branch, page))?,
            "branch rules",
        )?;
        for rule in rules
            .iter()
            .filter(|rule| rule.kind == "required_status_checks")
        {
            let listed = rule
                .parameters
                .as_ref()
                .and_then(|parameters| parameters.required_status_checks.as_ref())
                .ok_or_else(|| {
                    CiError::new(format!(
                        "a required_status_checks rule of branch {branch} lists no checks"
                    ))
                })?;
            for check in listed {
                checks.insert(check.context.clone());
                app_checks.push(check.clone());
            }
        }
        let last = rules.len() < RULES_PAGE;
        applicable.extend(rules);
        if last {
            break;
        }
    }
    for check in classic_checks(github, repo, branch)? {
        checks.insert(check.context.clone());
        app_checks.push(check);
    }
    if checks.is_empty() {
        return Err(CiError::new(format!(
            "branch {branch} requires no status check in its rules or its branch protection, so no commit of it can be released"
        )));
    }
    for name in pr_only_checks {
        if !checks.contains(name) {
            return Err(CiError::new(format!(
                "PR-only check {name} is not required on branch {branch}"
            )));
        }
    }
    if !pr_only_checks.is_empty() {
        exemptions::require_enforcement(
            github,
            repo,
            branch,
            &applicable,
            pr_only_checks,
            &app_checks,
        )?;
    }
    if checks.iter().all(|name| pr_only_checks.contains(name)) {
        return Err(CiError::new(format!(
            "branch {branch} has no required push check after PR-only exemptions"
        )));
    }
    for check in &app_checks {
        if !pr_only_checks.contains(&check.context) {
            require(branch, check)?;
        }
    }
    Ok(RequiredChecks {
        names: checks,
        exempt_sources: app_checks
            .into_iter()
            .filter(|check| pr_only_checks.contains(&check.context))
            .collect(),
    })
}

fn classic_checks(
    github: &dyn GitHub,
    repo: &Repo,
    branch: &str,
) -> Result<Vec<ApiRequiredCheck>, CiError> {
    let ApiBranch { protection } = parse(&github.get(&branch_path(repo, branch))?, "branch")?;
    let classic = protection.required_status_checks;
    if classic.enforcement_level == "off" {
        return Ok(Vec::new());
    }
    let mut checks = classic.checks;
    for context in classic.contexts {
        if !checks.iter().any(|check| check.context == context) {
            checks.push(ApiRequiredCheck {
                context,
                integration_id: None,
            });
        }
    }
    Ok(checks)
}

fn require(branch: &str, check: &ApiRequiredCheck) -> Result<(), CiError> {
    if let Some(app) = check
        .integration_id
        .filter(|app| ![ACTIONS_APP_ID, ANY_APP].contains(app))
    {
        return Err(CiError::new(format!(
            "branch {branch} requires check {} from the GitHub App {app}, and only a check from GitHub Actions ({ACTIONS_APP_ID}) can be tied to a push run",
            check.context
        )));
    }
    Ok(())
}
