//! Fail-closed resolution of GitHub's active branch check and bypass rules.

use serde_json::Value;
use velnor_actions_contract::homebrew_merge_gate::TapRequiredCheck;

use crate::{OrchestratorError, internal::internal};

use super::github::TapMergeGitHub;

#[path = "tap_merge_protection_rulesets.rs"]
mod rulesets;
use rulesets::{effective_branch_rules, load_effective_rulesets, parse_ruleset};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProtectionFacts {
    pub(super) strict: bool,
    pub(super) checks: Vec<(String, u64)>,
    pub(super) writer_bypass: bool,
}

pub(super) struct ProtectionAuthority<'a> {
    pub(super) writer_app_id: u64,
    pub(super) writer_bot_user_id: u64,
    pub(super) writer_bot_login: &'a str,
    pub(super) completion_check_name: &'a str,
    pub(super) completion_app_id: u64,
}

#[derive(Debug, Default)]
struct ClassicProtection {
    checks: Vec<(String, u64)>,
    strict: bool,
    writer_bypass: bool,
}

#[derive(Debug, Default)]
struct RulesetProtection {
    checks: Vec<(String, u64)>,
    writer_bypass: bool,
    strict: bool,
}

pub(super) fn resolve_protection(
    api: &TapMergeGitHub,
    repository: &str,
    branch: &str,
    authority: &ProtectionAuthority<'_>,
) -> Result<ProtectionFacts, OrchestratorError> {
    let encoded_branch = encode_segment(branch);
    let branch_protection: Option<Value> = api.get_optional(&format!(
        "repos/{repository}/branches/{encoded_branch}/protection"
    ))?;
    let legacy = branch_protection
        .as_ref()
        .map(|value| {
            parse_classic_protection(value, authority.writer_app_id, authority.writer_bot_login)
        })
        .transpose()?
        .unwrap_or_default();
    let mut checks = legacy.checks;
    let mut writer_bypass = legacy.writer_bypass;
    let effective_rules = effective_branch_rules(api, repository, &encoded_branch)?;
    let rulesets = load_effective_rulesets(api, &effective_rules, repository)?;
    let mut ruleset_strict = false;
    for ruleset in &rulesets {
        let parsed = parse_ruleset(
            ruleset,
            authority.writer_app_id,
            authority.writer_bot_user_id,
        )?;
        checks.extend(parsed.checks);
        writer_bypass |= parsed.writer_bypass;
        ruleset_strict |= parsed.strict;
    }
    checks.sort();
    checks.dedup();
    validate_required_check_bindings(
        &checks,
        authority.completion_check_name,
        authority.completion_app_id,
    )?;
    Ok(ProtectionFacts {
        strict: legacy.strict || ruleset_strict,
        checks,
        writer_bypass,
    })
}

fn validate_required_check_bindings(
    checks: &[(String, u64)],
    completion_check_name: &str,
    completion_app_id: u64,
) -> Result<(), OrchestratorError> {
    if !checks.contains(&(completion_check_name.to_owned(), completion_app_id)) {
        return Err(internal("tap_merge_completion_check_not_protected"));
    }
    if !checks.iter().any(|(name, _)| name == "DCO") {
        return Err(internal("tap_merge_required_checks_or_dco_missing"));
    }
    Ok(())
}

pub(super) fn exact_required_checks(
    facts: &ProtectionFacts,
    check_runs: &[Value],
    head_sha: &str,
) -> Result<Option<Vec<TapRequiredCheck>>, OrchestratorError> {
    if facts.checks.is_empty() {
        return Err(internal("tap_merge_required_checks_missing"));
    }
    let mut resolved = Vec::with_capacity(facts.checks.len());
    let mut pending = false;
    for (name, app_id) in &facts.checks {
        let matches = check_runs
            .iter()
            .filter(|run| run["name"].as_str() == Some(name))
            .filter(|run| run.pointer("/app/id").and_then(Value::as_u64) == Some(*app_id))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            pending = true;
            continue;
        }
        if matches.len() != 1 {
            return Err(internal("tap_merge_required_check_ambiguous"));
        }
        let run = matches[0];
        let status = run["status"]
            .as_str()
            .ok_or_else(|| internal("tap_merge_required_check_status_invalid"))?;
        if status != "completed" {
            if !matches!(status, "queued" | "in_progress")
                || (!run["conclusion"].is_null() && run["conclusion"].as_str() != Some("success"))
            {
                return Err(internal("tap_merge_required_check_failed"));
            }
            pending = true;
            continue;
        }
        if run["conclusion"].as_str() != Some("success")
            || run["head_sha"].as_str() != Some(head_sha)
        {
            return Err(internal("tap_merge_required_check_failed"));
        }
        resolved.push(TapRequiredCheck {
            name: name.clone(),
            app_id: *app_id,
            required_app_id: Some(*app_id),
            head_sha: run["head_sha"]
                .as_str()
                .ok_or_else(|| internal("tap_merge_required_check_head"))?
                .to_owned(),
            completed_successfully: true,
        });
    }
    Ok((!pending).then_some(resolved))
}

fn parse_classic_protection(
    value: &Value,
    writer_app_id: u64,
    writer_bot_login: &str,
) -> Result<ClassicProtection, OrchestratorError> {
    if value
        .pointer("/enforce_admins/enabled")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(internal("tap_merge_admin_protection_disabled"));
    }
    let review = value
        .get("required_pull_request_reviews")
        .filter(|review| !review.is_null())
        .ok_or_else(|| internal("tap_merge_classic_review_protection_missing"))?;
    let allowances = review
        .get("bypass_pull_request_allowances")
        .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
    let mut writer_bypass = false;
    let apps = allowances["apps"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
    for app in apps {
        let app_id = app["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
        writer_bypass |= app_id == writer_app_id;
    }
    let users = allowances["users"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
    for user in users {
        let login = user["login"]
            .as_str()
            .filter(|login| !login.is_empty())
            .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
        writer_bypass |= login.eq_ignore_ascii_case(writer_bot_login);
    }
    let teams = allowances["teams"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
    // Team membership is not exposed to this credential boundary. Any configured
    // team allowance is therefore ambiguous with respect to the writer bot.
    writer_bypass |= !teams.is_empty();
    let Some(status_checks) = value.get("required_status_checks") else {
        return Ok(ClassicProtection {
            writer_bypass,
            ..ClassicProtection::default()
        });
    };
    if status_checks.is_null() {
        return Ok(ClassicProtection {
            writer_bypass,
            ..ClassicProtection::default()
        });
    }
    let strict = status_checks.get("strict").and_then(Value::as_bool) == Some(true);
    let raw = status_checks
        .get("checks")
        .and_then(Value::as_array)
        .ok_or_else(|| internal("tap_merge_protection_checks_unreadable"))?;
    let checks = raw
        .iter()
        .map(|entry| {
            let name = entry["context"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| internal("tap_merge_protection_context_invalid"))?;
            let app = entry["app_id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| internal("tap_merge_protection_app_unbound"))?;
            Ok::<_, OrchestratorError>((name.to_owned(), app))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ClassicProtection {
        checks,
        strict,
        writer_bypass,
    })
}

fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

#[cfg(test)]
#[path = "tap_merge_protection_tests.rs"]
mod tests;
