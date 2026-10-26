// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::MiseError;

#[must_use]
pub fn pr_save_allowed(action_supports_pr_save: bool, is_fork: bool, event: &str) -> bool {
    if is_fork {
        return false;
    }
    if event != "pull_request" {
        return false;
    }
    action_supports_pr_save
}

#[must_use]
pub fn is_read_only(is_fork: bool) -> bool {
    is_fork
}

#[must_use]
pub fn pr_outputs_trusted(trust: &str) -> bool {
    trust == "trusted"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "P08 names four independent save conditions; folding them hides the policy"
)]
pub struct SaveGate {
    pub producer_passed: bool,
    pub useful_delta: bool,
    pub trust_scope_ok: bool,
    pub writers_finished: bool,
}

#[must_use]
pub fn save_after_success(gate: SaveGate) -> bool {
    gate.producer_passed && gate.useful_delta && gate.trust_scope_ok && gate.writers_finished
}

pub fn authorize_trusted_save() -> Result<&'static str, MiseError> {
    use crate::restore::{SaveInputs, save_decision};
    let open = SaveGate {
        producer_passed: true,
        useful_delta: true,
        trust_scope_ok: true,
        writers_finished: true,
    };
    if !save_after_success(open) {
        return Err(contract("save_gate_weakened"));
    }
    let allowed = SaveInputs {
        layer_trust: "trusted",
        event: "push",
        passed: true,
        unavailable: false,
        active_writer: false,
    };
    if save_decision(&allowed).is_err() {
        return Err(contract("save_policy_drift:push_denied"));
    }
    deny_non_push_variants(&allowed)?;
    for denied in [
        SaveInputs {
            passed: false,
            ..allowed
        },
        SaveInputs {
            unavailable: true,
            ..allowed
        },
        SaveInputs {
            active_writer: true,
            ..allowed
        },
    ] {
        if save_decision(&denied).is_ok() {
            return Err(contract("save_policy_drift:overpermissive"));
        }
    }
    if !crate::cache::save_allowed("trusted", "push", true)
        || crate::cache::save_allowed("trusted", "push", false)
    {
        return Err(contract("save_policy_drift:allowlist"));
    }
    let gate = velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
    if gate != "success() && github.event_name == 'push'" {
        return Err(contract("save_gate_not_push_only"));
    }
    Ok(gate)
}

fn deny_non_push_variants(allowed: &crate::restore::SaveInputs<'_>) -> Result<(), MiseError> {
    use crate::restore::save_decision;
    for event in [
        "pull_request",
        "pull_request_target",
        "merge_group",
        "schedule",
        "workflow_dispatch",
        "workflow_call",
        "",
        "Push",
    ] {
        let denied = crate::restore::SaveInputs { event, ..*allowed };
        if save_decision(&denied).is_ok() {
            return Err(contract("save_policy_drift:event_allowed"));
        }
        if crate::cache::save_allowed("trusted", event, true) {
            return Err(contract("save_policy_drift:allowlist"));
        }
    }
    for layer_trust in ["", "untrusted", "prerelease"] {
        let denied = crate::restore::SaveInputs {
            layer_trust,
            ..*allowed
        };
        if save_decision(&denied).is_ok() {
            return Err(contract("save_policy_drift:trust_allowed"));
        }
    }
    Ok(())
}

#[must_use]
pub fn cache_error_fails_verification(_cache_error: bool, _task_passed: bool) -> bool {
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheUsage {
    pub active_bytes: u64,
    pub count: u64,
}

pub fn parse_service_usage(json: &str) -> Result<CacheUsage, MiseError> {
    let trimmed = json.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return Err(contract("usage_not_array"));
    }
    let mut active_bytes: u64 = 0;
    let mut count: u64 = 0;
    let mut rest = trimmed;
    while let Some(at) = rest.find("sizeInBytes") {
        rest = &rest[at + "sizeInBytes".len()..];
        let digits: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.is_empty() {
            break;
        }
        let skip = rest
            .find(&digits)
            .map_or(rest.len(), |at| at + digits.len());
        rest = &rest[skip.min(rest.len())..];
        match digits.parse::<u64>() {
            Ok(num) => {
                active_bytes = active_bytes.saturating_add(num);
                count += 1;
            }
            Err(_) => return Err(contract("usage_bad_size")),
        }
    }
    Ok(CacheUsage {
        active_bytes,
        count,
    })
}

pub fn headroom_bytes(active_bytes: u64, limit_bytes: u64) -> Result<u64, MiseError> {
    if active_bytes <= limit_bytes {
        Ok(limit_bytes - active_bytes)
    } else {
        Err(MiseError::InvalidStepInput {
            field: "cache_quota".to_owned(),
            value: format!("{active_bytes}>{limit_bytes}"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheUsageReport {
    pub active_bytes: u64,
    pub count: u64,
    pub limit_bytes: u64,
    pub headroom_bytes: u64,
    pub stored_bytes: u64,
    pub restoring_jobs: u64,
    pub aggregate_transfer_bytes: u64,
}

pub fn summarize_cache_usage(
    json: &str,
    limit_bytes: u64,
    stored_bytes: u64,
    restoring_jobs: u64,
) -> Result<CacheUsageReport, MiseError> {
    let usage = parse_service_usage(json)?;
    let headroom = headroom_bytes(usage.active_bytes, limit_bytes)?;
    let (stored, transfer) = stored_vs_transfer(stored_bytes, restoring_jobs);
    Ok(CacheUsageReport {
        active_bytes: usage.active_bytes,
        count: usage.count,
        limit_bytes,
        headroom_bytes: headroom,
        stored_bytes: stored,
        restoring_jobs: restoring_jobs.max(1),
        aggregate_transfer_bytes: transfer,
    })
}

#[must_use]
pub fn stored_vs_transfer(stored_bytes: u64, restoring_jobs: u64) -> (u64, u64) {
    (
        stored_bytes,
        stored_bytes.saturating_mul(restoring_jobs.max(1)),
    )
}

fn contract(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}
