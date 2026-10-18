//! Validation of GitHub branch and deployment environment settings.

use serde_json::Value;
use std::collections::BTreeSet;

use super::STABLE_ENVIRONMENT;

pub(super) fn validate_effective_main_rules(value: &Value) -> Result<(), ()> {
    let rules = value.as_array().ok_or(())?;
    let mut pull_request = false;
    let mut status_checks = false;
    let mut no_force_push = false;
    let mut no_deletion = false;

    for rule in rules {
        let Some(kind) = rule.get("type").and_then(Value::as_str) else {
            return Err(());
        };
        match kind {
            "pull_request" => {
                let params = rule.get("parameters").ok_or(())?;
                let approvals = params
                    .get("required_approving_review_count")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                let thread_resolution = params
                    .get("required_review_thread_resolution")
                    .and_then(Value::as_bool)
                    .ok_or(())?;
                if approvals > 100 || !thread_resolution {
                    return Err(());
                }
                pull_request = true;
            }
            "required_status_checks" => {
                let checks = rule
                    .get("parameters")
                    .and_then(|params| params.get("required_status_checks"))
                    .and_then(Value::as_array)
                    .ok_or(())?;
                if checks.is_empty()
                    || checks.iter().any(|check| {
                        check
                            .get("context")
                            .and_then(Value::as_str)
                            .is_none_or(str::is_empty)
                    })
                {
                    return Err(());
                }
                status_checks = true;
            }
            "non_fast_forward" => no_force_push = true,
            "deletion" => no_deletion = true,
            _ => {}
        }
    }

    if pull_request && status_checks && no_force_push && no_deletion {
        Ok(())
    } else {
        Err(())
    }
}

pub(super) fn validate_main_protection_sources(
    rules: &Value,
    classic: Option<&Value>,
) -> Result<(), ()> {
    let rules_result = validate_effective_main_rules(rules);
    if rules_result.is_ok() {
        return Ok(());
    }
    let classic = classic.ok_or(())?;
    let facts = MainProtectionFacts::from_classic(classic)?;
    let rules_facts = MainProtectionFacts::from_rules(rules)?;
    if facts.merge(rules_facts).is_complete() {
        Ok(())
    } else {
        Err(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ProtectionControl {
    PullRequest,
    StatusChecks,
    NoForcePush,
    NoDeletion,
}

#[derive(Default)]
struct MainProtectionFacts {
    controls: BTreeSet<ProtectionControl>,
}

impl MainProtectionFacts {
    fn from_rules(rules: &Value) -> Result<Self, ()> {
        let rules = rules.as_array().ok_or(())?;
        let mut facts = Self::default();
        for rule in rules {
            let Some(kind) = rule.get("type").and_then(Value::as_str) else {
                return Err(());
            };
            match kind {
                "pull_request" => {
                    let params = rule.get("parameters").ok_or(())?;
                    let approvals = params
                        .get("required_approving_review_count")
                        .and_then(Value::as_u64)
                        .ok_or(())?;
                    let threads = params
                        .get("required_review_thread_resolution")
                        .and_then(Value::as_bool)
                        .ok_or(())?;
                    if approvals > 100 || !threads {
                        return Err(());
                    }
                    facts.controls.insert(ProtectionControl::PullRequest);
                }
                "required_status_checks" => {
                    let checks = rule
                        .get("parameters")
                        .and_then(|params| params.get("required_status_checks"))
                        .and_then(Value::as_array)
                        .ok_or(())?;
                    if checks.is_empty()
                        || checks.iter().any(|check| {
                            check
                                .get("context")
                                .and_then(Value::as_str)
                                .is_none_or(str::is_empty)
                        })
                    {
                        return Err(());
                    }
                    facts.controls.insert(ProtectionControl::StatusChecks);
                }
                "non_fast_forward" => {
                    facts.controls.insert(ProtectionControl::NoForcePush);
                }
                "deletion" => {
                    facts.controls.insert(ProtectionControl::NoDeletion);
                }
                _ => {}
            }
        }
        Ok(facts)
    }

    fn from_classic(classic: &Value) -> Result<Self, ()> {
        let approvals = classic
            .get("required_pull_request_reviews")
            .and_then(|reviews| reviews.get("required_approving_review_count"))
            .and_then(Value::as_u64)
            .ok_or(())?;
        let threads = classic
            .get("required_conversation_resolution")
            .and_then(|resolution| resolution.get("enabled"))
            .and_then(Value::as_bool)
            .ok_or(())?;
        let status_checks = classic_status_checks_present(classic)?;
        let no_force_push = classic
            .get("allow_force_pushes")
            .and_then(|setting| setting.get("enabled"))
            .and_then(Value::as_bool)
            == Some(false);
        let no_deletion = classic
            .get("allow_deletions")
            .and_then(|setting| setting.get("enabled"))
            .and_then(Value::as_bool)
            == Some(false);
        Ok(Self {
            controls: [
                (approvals <= 100 && threads).then_some(ProtectionControl::PullRequest),
                status_checks.then_some(ProtectionControl::StatusChecks),
                no_force_push.then_some(ProtectionControl::NoForcePush),
                no_deletion.then_some(ProtectionControl::NoDeletion),
            ]
            .into_iter()
            .flatten()
            .collect(),
        })
    }

    fn merge(mut self, other: Self) -> Self {
        self.controls.extend(other.controls);
        self
    }

    fn is_complete(&self) -> bool {
        self.controls.len() == 4
    }
}

fn classic_status_checks_present(classic: &Value) -> Result<bool, ()> {
    let Some(checks) = classic.get("required_status_checks") else {
        return Ok(false);
    };
    let contexts = checks.get("contexts").and_then(Value::as_array).ok_or(())?;
    Ok(!contexts.is_empty()
        && contexts
            .iter()
            .all(|context| context.as_str().is_some_and(|name| !name.is_empty())))
}

pub(super) fn validate_stable_environment_settings(
    settings: &Value,
    branch: &str,
) -> Result<(), ()> {
    if branch != "main"
        || settings.get("name").and_then(Value::as_str) != Some(STABLE_ENVIRONMENT)
        || settings
            .get("deployment_branch_policy")
            .and_then(|policy| policy.get("protected_branches"))
            .and_then(Value::as_bool)
            != Some(true)
        || settings
            .get("deployment_branch_policy")
            .and_then(|policy| policy.get("custom_branch_policies"))
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err(());
    }
    let reviewers = settings
        .get("protection_rules")
        .and_then(Value::as_array)
        .ok_or(())?;
    if reviewers.len() != 1 {
        return Err(());
    }
    let rule = &reviewers[0];
    if rule.get("type").and_then(Value::as_str) != Some("required_reviewers")
        || rule.get("prevent_self_review").and_then(Value::as_bool) != Some(true)
        || rule
            .get("reviewers")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        return Err(());
    }
    Ok(())
}

pub(super) fn validate_environment_settings(
    settings: &Value,
    expected_name: &str,
) -> Result<(), ()> {
    if settings.get("name").and_then(Value::as_str) != Some(expected_name) {
        return Err(());
    }
    if settings
        .get("protection_rules")
        .and_then(Value::as_array)
        .is_none_or(|rules| !rules.is_empty())
    {
        return Err(());
    }
    let policy = settings.get("deployment_branch_policy").ok_or(())?;
    if policy.get("protected_branches").and_then(Value::as_bool) != Some(false)
        || policy
            .get("custom_branch_policies")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(());
    }
    Ok(())
}

pub(super) fn validate_environment_branch_policy(policies: &Value, branch: &str) -> Result<(), ()> {
    if branch != "main" || policies.get("total_count").and_then(Value::as_u64) != Some(1) {
        return Err(());
    }
    let entries = policies
        .get("branch_policies")
        .and_then(Value::as_array)
        .ok_or(())?;
    if entries.len() != 1 || entries[0].get("name").and_then(Value::as_str) != Some(branch) {
        return Err(());
    }
    Ok(())
}
