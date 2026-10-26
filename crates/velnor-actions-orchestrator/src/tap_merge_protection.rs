// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;
use velnor_actions_contract::homebrew_merge_gate::TapRequiredCheck;

use crate::{OrchestratorError, internal::internal};

use super::github::TapMergeGitHub;

#[path = "tap_merge_protection_rulesets.rs"]
mod rulesets;
use rulesets::{effective_branch_rules, load_effective_rulesets, parse_ruleset};

#[path = "tap_merge_protection_classic.rs"]
mod classic;
use classic::parse_classic_protection;

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
    admins_enforced: bool,
    controls: BranchControls,
}

#[derive(Debug, Default)]
struct RulesetProtection {
    checks: Vec<(String, u64)>,
    writer_bypass: bool,
    strict: bool,
    controls: BranchControls,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum BranchControl {
    PullRequest,
    ThreadResolution,
    NoForcePush,
    NoDeletion,
}

#[derive(Debug, Default)]
struct BranchControls(BTreeSet<BranchControl>);

impl BranchControls {
    fn insert(&mut self, control: BranchControl) {
        self.0.insert(control);
    }

    fn extend(&mut self, other: &Self) {
        self.0.extend(other.0.iter().copied());
    }

    fn contains(&self, control: BranchControl) -> bool {
        self.0.contains(&control)
    }

    fn contains_required_controls(&self) -> bool {
        [
            BranchControl::PullRequest,
            BranchControl::ThreadResolution,
            BranchControl::NoForcePush,
            BranchControl::NoDeletion,
        ]
        .into_iter()
        .all(|control| self.contains(control))
    }
}

pub(super) fn resolve_protection(
    api: &TapMergeGitHub,
    observer: &impl super::protection_observer::ClassicProtectionReader,
    repository: &str,
    branch: &str,
    authority: &ProtectionAuthority<'_>,
) -> Result<ProtectionFacts, OrchestratorError> {
    let encoded_branch = encode_segment(branch);
    let branch_protection = observer.classic_protection(repository, &encoded_branch)?;
    let classic_present = branch_protection.is_some();
    let legacy = branch_protection
        .as_ref()
        .map(|value| {
            parse_classic_protection(value, authority.writer_app_id, authority.writer_bot_login)
        })
        .transpose()?
        .unwrap_or_default();
    let mut checks = legacy.checks.clone();
    let mut writer_bypass = legacy.writer_bypass;
    let mut ruleset_checks = Vec::new();
    let effective_rules = effective_branch_rules(api, repository, &encoded_branch)?;
    let rulesets = load_effective_rulesets(api, &effective_rules, repository)?;
    let mut ruleset_strict = false;
    let mut ruleset_controls = BranchControls::default();
    for ruleset in &rulesets {
        let parsed = parse_ruleset(
            ruleset,
            authority.writer_app_id,
            authority.writer_bot_user_id,
        )?;
        ruleset_checks.extend(parsed.checks.iter().cloned());
        ruleset_controls.extend(&parsed.controls);
        checks.extend(parsed.checks);
        writer_bypass |= parsed.writer_bypass;
        ruleset_strict |= parsed.strict;
    }
    let admin_rule_coverage = ruleset_strict
        && ruleset_controls.contains_required_controls()
        && !writer_bypass
        && ruleset_checks.iter().any(|(name, _)| name == "DCO")
        && ruleset_checks.contains(&(
            authority.completion_check_name.to_owned(),
            authority.completion_app_id,
        ))
        && legacy
            .checks
            .iter()
            .all(|check| ruleset_checks.contains(check));
    if classic_present && !legacy.admins_enforced && !admin_rule_coverage {
        return Err(internal("tap_merge_admin_protection_unproven"));
    }
    let mut combined_controls = BranchControls::default();
    combined_controls.extend(&legacy.controls);
    combined_controls.extend(&ruleset_controls);
    if !combined_controls.contains_required_controls() || !(legacy.strict || ruleset_strict) {
        return Err(internal("tap_merge_branch_controls_incomplete"));
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
