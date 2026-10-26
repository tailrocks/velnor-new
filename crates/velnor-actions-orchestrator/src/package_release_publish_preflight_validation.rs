// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use std::collections::BTreeSet;

use super::STABLE_ENVIRONMENT;
use velnor_actions_contract::is_valid_branch_name;

pub(super) fn validate_main_protection_sources(
    rules: &Value,
    classic: Option<&Value>,
) -> Result<(), ()> {
    let rules_facts = MainProtectionFacts::from_rules(rules)?;
    let classic_facts = classic
        .map(MainProtectionFacts::from_classic)
        .transpose()?
        .unwrap_or_default();
    rules_facts.merge(classic_facts).complete()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ProtectionControl {
    PullRequest,
    ThreadResolution,
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
                "pull_request" => facts.controls.extend(ruleset_pull_request_controls(rule)?),
                "required_status_checks" => {
                    if ruleset_status_checks_present(rule)? {
                        facts.controls.insert(ProtectionControl::StatusChecks);
                    }
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
        let mut facts = Self::default();
        if classic_approvals(classic)?.is_some() {
            facts.controls.insert(ProtectionControl::PullRequest);
        }
        if classic_thread_resolution(classic)? {
            facts.controls.insert(ProtectionControl::ThreadResolution);
        }
        if classic_status_checks_present(classic)? {
            facts.controls.insert(ProtectionControl::StatusChecks);
        }
        if classic_prohibits(classic, "allow_force_pushes")? {
            facts.controls.insert(ProtectionControl::NoForcePush);
        }
        if classic_prohibits(classic, "allow_deletions")? {
            facts.controls.insert(ProtectionControl::NoDeletion);
        }
        Ok(facts)
    }

    fn merge(mut self, other: Self) -> Self {
        self.controls.extend(other.controls);
        self
    }

    fn is_complete(&self) -> bool {
        self.controls.len() == 5
    }

    fn complete(&self) -> Result<(), ()> {
        if self.is_complete() { Ok(()) } else { Err(()) }
    }
}

fn ruleset_pull_request_controls(rule: &Value) -> Result<Vec<ProtectionControl>, ()> {
    let params = rule
        .get("parameters")
        .and_then(Value::as_object)
        .ok_or(())?;
    let approvals = params
        .get("required_approving_review_count")
        .and_then(Value::as_u64)
        .ok_or(())?;
    let mut controls = Vec::new();
    if approvals <= 100 {
        controls.push(ProtectionControl::PullRequest);
    } else {
        return Err(());
    }
    if params
        .get("required_review_thread_resolution")
        .and_then(Value::as_bool)
        .ok_or(())?
    {
        controls.push(ProtectionControl::ThreadResolution);
    }
    Ok(controls)
}

fn ruleset_status_checks_present(rule: &Value) -> Result<bool, ()> {
    let params = rule
        .get("parameters")
        .and_then(Value::as_object)
        .ok_or(())?;
    let checks = params
        .get("required_status_checks")
        .and_then(Value::as_array)
        .ok_or(())?;
    if checks.iter().any(|check| {
        check
            .get("context")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    }) {
        return Err(());
    }
    Ok(!checks.is_empty())
}

fn classic_approvals(classic: &Value) -> Result<Option<u64>, ()> {
    classic.as_object().ok_or(())?;
    let Some(value) = classic.get("required_pull_request_reviews") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let reviews = value.as_object().ok_or(())?;
    let count = Some(
        reviews
            .get("required_approving_review_count")
            .and_then(Value::as_u64)
            .ok_or(())?,
    );
    if count.is_some_and(|value| value > 100) {
        return Err(());
    }
    Ok(count)
}

fn classic_thread_resolution(classic: &Value) -> Result<bool, ()> {
    classic.as_object().ok_or(())?;
    let Some(value) = classic.get("required_conversation_resolution") else {
        return Ok(false);
    };
    if value.is_null() {
        return Ok(false);
    }
    let enabled = value.get("enabled").ok_or(())?.as_bool().ok_or(())?;
    Ok(enabled)
}

fn classic_prohibits(classic: &Value, key: &str) -> Result<bool, ()> {
    classic.as_object().ok_or(())?;
    let Some(value) = classic.get(key) else {
        return Ok(false);
    };
    if value.is_null() {
        return Ok(false);
    }
    let enabled = value.get("enabled").ok_or(())?.as_bool().ok_or(())?;
    Ok(!enabled)
}

fn classic_status_checks_present(classic: &Value) -> Result<bool, ()> {
    classic.as_object().ok_or(())?;
    let Some(checks) = classic.get("required_status_checks") else {
        return Ok(false);
    };
    if checks.is_null() {
        return Ok(false);
    }
    let contexts = checks.get("contexts").and_then(Value::as_array).ok_or(())?;
    if contexts
        .iter()
        .any(|context| context.as_str().is_none_or(str::is_empty))
    {
        return Err(());
    }
    Ok(!contexts.is_empty())
}

pub(super) fn validate_stable_environment_settings(
    settings: &Value,
    branch: &str,
) -> Result<(), ()> {
    validate_human_environment_settings(settings, branch, STABLE_ENVIRONMENT)
}

pub(super) fn validate_human_environment_settings(
    settings: &Value,
    branch: &str,
    expected_name: &str,
) -> Result<(), ()> {
    if !is_valid_branch_name(branch)
        || settings.get("name").and_then(Value::as_str) != Some(expected_name)
        || settings.get("can_admins_bypass").and_then(Value::as_bool) != Some(false)
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
    let mut required_human_review = false;
    for rule in reviewers {
        let kind = rule.get("type").and_then(Value::as_str).ok_or(())?;
        if kind == "required_reviewers" {
            let entries = rule.get("reviewers").and_then(Value::as_array).ok_or(())?;
            if rule.get("prevent_self_review").and_then(Value::as_bool) != Some(true)
                || entries.is_empty()
                || entries
                    .iter()
                    .any(|entry| !valid_environment_reviewer(entry))
            {
                return Err(());
            }
            required_human_review = true;
        }
    }
    if !required_human_review {
        return Err(());
    }
    Ok(())
}

fn valid_environment_reviewer(value: &Value) -> bool {
    matches!(
        value.get("type").and_then(Value::as_str),
        Some("User" | "Team")
    ) && value
        .get("reviewer")
        .and_then(|reviewer| reviewer.get("id"))
        .and_then(Value::as_u64)
        .is_some_and(|id| id > 0)
}

pub(super) fn validate_environment_settings(
    settings: &Value,
    expected_name: &str,
) -> Result<(), ()> {
    if settings.get("name").and_then(Value::as_str) != Some(expected_name) {
        return Err(());
    }
    let rules = settings
        .get("protection_rules")
        .and_then(Value::as_array)
        .ok_or(())?;
    if rules.len() > 1
        || rules
            .iter()
            .any(|rule| rule.get("type").and_then(Value::as_str) != Some("branch_policy"))
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
