//! Resolve GitHub's effective branch rules to their full ruleset records.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

use super::super::github::TapMergeGitHub;
use super::RulesetProtection;

const RULES_PAGE_SIZE: usize = 100;
const MAX_RULE_PAGES: u64 = 100;

pub(super) fn effective_branch_rules(
    api: &TapMergeGitHub,
    repository: &str,
    encoded_branch: &str,
) -> Result<Vec<Value>, OrchestratorError> {
    let mut rules = Vec::new();
    for page in 1..=MAX_RULE_PAGES {
        let current: Vec<Value> = api.get(&format!(
            "repos/{repository}/rules/branches/{encoded_branch}?per_page={RULES_PAGE_SIZE}&page={page}"
        ))?;
        if current.len() > RULES_PAGE_SIZE {
            return Err(internal("tap_merge_rules_page_oversized"));
        }
        let count = current.len();
        rules.extend(current);
        if count < RULES_PAGE_SIZE {
            return Ok(rules);
        }
    }
    Err(internal("tap_merge_rules_page_limit"))
}

pub(super) fn load_effective_rulesets(
    api: &TapMergeGitHub,
    effective_rules: &[Value],
    repository: &str,
) -> Result<Vec<Value>, OrchestratorError> {
    let mut sources = BTreeMap::<u64, (String, String)>::new();
    for rule in effective_rules {
        let id = rule["ruleset_id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_ruleset_id_invalid"))?;
        let source_type = rule["ruleset_source_type"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("tap_merge_ruleset_source_invalid"))?;
        let source = rule["ruleset_source"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("tap_merge_ruleset_source_invalid"))?;
        let current = (source_type.to_owned(), source.to_owned());
        if sources
            .insert(id, current.clone())
            .is_some_and(|old| old != current)
        {
            return Err(internal("tap_merge_ruleset_source_conflict"));
        }
    }
    sources
        .into_iter()
        .map(|(id, (source_type, source))| {
            let route = ruleset_detail_route(repository, id, &source_type, &source)?;
            let detail: Value = api.get(&route)?;
            if detail["id"].as_u64() != Some(id)
                || detail["source_type"].as_str() != Some(source_type.as_str())
                || detail["source"].as_str() != Some(source.as_str())
            {
                return Err(internal("tap_merge_ruleset_identity_mismatch"));
            }
            Ok(detail)
        })
        .collect()
}

pub(super) fn ruleset_detail_route(
    repository: &str,
    id: u64,
    source_type: &str,
    source: &str,
) -> Result<String, OrchestratorError> {
    match source_type {
        "Repository" if source == repository => Ok(format!(
            "repos/{repository}/rulesets/{id}?includes_parents=true"
        )),
        "Organization" if safe_owner(source) => Ok(format!("orgs/{source}/rulesets/{id}")),
        "Enterprise" if safe_owner(source) => Ok(format!("enterprises/{source}/rulesets/{id}")),
        _ => Err(internal("tap_merge_ruleset_source_untrusted")),
    }
}

fn safe_owner(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub(super) fn parse_ruleset(
    ruleset: &Value,
    writer_app_id: u64,
    writer_bot_user_id: u64,
) -> Result<RulesetProtection, OrchestratorError> {
    if ruleset["target"].as_str() != Some("branch")
        || ruleset["enforcement"].as_str() != Some("active")
    {
        return Err(internal("tap_merge_ruleset_not_active"));
    }
    let bypass = ruleset["bypass_actors"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_ruleset_bypass_unreadable"))?;
    let mut writer_bypass = false;
    for actor in bypass {
        let kind = actor["actor_type"]
            .as_str()
            .ok_or_else(|| internal("tap_merge_ruleset_bypass_invalid"))?;
        match kind {
            "Integration" => {
                let id = actor["actor_id"]
                    .as_u64()
                    .filter(|id| *id > 0)
                    .ok_or_else(|| internal("tap_merge_ruleset_bypass_invalid"))?;
                writer_bypass |= id == writer_app_id;
            }
            "User" => {
                let id = actor["actor_id"]
                    .as_u64()
                    .filter(|id| *id > 0)
                    .ok_or_else(|| internal("tap_merge_ruleset_bypass_invalid"))?;
                writer_bypass |= id == writer_bot_user_id;
            }
            // These identities cannot be proved distinct from the writer with the
            // available token and response fields. Fail closed rather than assuming.
            "Team" | "OrganizationAdmin" | "DeployKey" => {
                return Err(internal("tap_merge_ruleset_bypass_identity_unresolved"));
            }
            _ => return Err(internal("tap_merge_ruleset_bypass_invalid")),
        }
    }
    let mut checks = Vec::new();
    let mut strict_checks = false;
    let mut pull_request = false;
    let rules = ruleset["rules"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_ruleset_rules_unreadable"))?;
    for rule in rules {
        match rule["type"].as_str() {
            Some("pull_request") => pull_request |= valid_pull_request_rule(rule)?,
            Some("required_status_checks") => {
                strict_checks |= parse_required_status_checks(rule, &mut checks)?;
            }
            _ => {}
        }
    }
    Ok(RulesetProtection {
        checks,
        writer_bypass,
        strict: strict_checks,
        pull_request,
    })
}

fn valid_pull_request_rule(rule: &Value) -> Result<bool, OrchestratorError> {
    let params = rule
        .get("parameters")
        .and_then(Value::as_object)
        .ok_or_else(|| internal("tap_merge_ruleset_pull_request_unreadable"))?;
    let approvals = params
        .get("required_approving_review_count")
        .and_then(Value::as_u64)
        .filter(|count| *count <= 100)
        .ok_or_else(|| internal("tap_merge_ruleset_pull_request_unreadable"))?;
    let threads = params
        .get("required_review_thread_resolution")
        .and_then(Value::as_bool)
        .ok_or_else(|| internal("tap_merge_ruleset_pull_request_unreadable"))?;
    Ok(approvals <= 100 && threads)
}

fn parse_required_status_checks(
    rule: &Value,
    checks: &mut Vec<(String, u64)>,
) -> Result<bool, OrchestratorError> {
    let strict = rule
        .pointer("/parameters/strict_required_status_checks_policy")
        .and_then(Value::as_bool)
        .ok_or_else(|| internal("tap_merge_ruleset_strictness_unreadable"))?;
    let contexts = rule
        .pointer("/parameters/required_status_checks")
        .and_then(Value::as_array)
        .ok_or_else(|| internal("tap_merge_ruleset_checks_unreadable"))?;
    for check in contexts {
        let name = check["context"]
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| internal("tap_merge_ruleset_context_invalid"))?;
        let app_id = check["integration_id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_ruleset_app_unbound"))?;
        checks.push((name.to_owned(), app_id));
    }
    Ok(strict && !contexts.is_empty())
}
