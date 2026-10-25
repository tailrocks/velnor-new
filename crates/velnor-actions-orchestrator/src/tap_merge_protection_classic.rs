use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

use super::{BranchControl, BranchControls, ClassicProtection};

pub(super) fn parse_classic_protection(
    value: &Value,
    writer_app_id: u64,
    writer_bot_login: &str,
) -> Result<ClassicProtection, OrchestratorError> {
    let admins_enforced = value
        .pointer("/enforce_admins/enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| internal("tap_merge_admin_protection_unreadable"))?;
    let writer_bypass = classic_writer_bypass(value, writer_app_id, writer_bot_login)?;
    let (pull_request, thread_resolution) = classic_pr_controls(value)?;
    let no_force_push = classic_disallows(value, "allow_force_pushes")?;
    let no_deletion = classic_disallows(value, "allow_deletions")?;
    let controls = classic_controls([
        (pull_request, BranchControl::PullRequest),
        (thread_resolution, BranchControl::ThreadResolution),
        (no_force_push, BranchControl::NoForcePush),
        (no_deletion, BranchControl::NoDeletion),
    ]);
    let Some(status_checks) = value.get("required_status_checks") else {
        return Ok(ClassicProtection {
            writer_bypass,
            admins_enforced,
            controls,
            ..ClassicProtection::default()
        });
    };
    if status_checks.is_null() {
        return Ok(ClassicProtection {
            writer_bypass,
            admins_enforced,
            controls,
            ..ClassicProtection::default()
        });
    }
    let strict = status_checks
        .get("strict")
        .and_then(Value::as_bool)
        .ok_or_else(|| internal("tap_merge_protection_strictness_unreadable"))?;
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
    let strict = strict && !checks.is_empty();
    Ok(ClassicProtection {
        checks,
        strict,
        writer_bypass,
        admins_enforced,
        controls,
    })
}

fn classic_controls(values: impl IntoIterator<Item = (bool, BranchControl)>) -> BranchControls {
    let mut controls = BranchControls::default();
    for (enabled, control) in values {
        if enabled {
            controls.insert(control);
        }
    }
    controls
}

fn classic_pr_controls(value: &Value) -> Result<(bool, bool), OrchestratorError> {
    let pull_request = match value.get("required_pull_request_reviews") {
        None | Some(Value::Null) => false,
        Some(review) => {
            review
                .get("required_approving_review_count")
                .and_then(Value::as_u64)
                .filter(|count| *count <= 100)
                .ok_or_else(|| internal("tap_merge_classic_review_unreadable"))?;
            true
        }
    };
    let thread_resolution = match value.get("required_conversation_resolution") {
        None | Some(Value::Null) => false,
        Some(resolution) => resolution
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or_else(|| internal("tap_merge_classic_thread_resolution_unreadable"))?,
    };
    Ok((pull_request, thread_resolution))
}

fn classic_disallows(value: &Value, key: &str) -> Result<bool, OrchestratorError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(setting) => setting
            .get("enabled")
            .and_then(Value::as_bool)
            .map(|enabled| !enabled)
            .ok_or_else(|| internal("tap_merge_classic_branch_control_unreadable")),
    }
}

fn classic_writer_bypass(
    value: &Value,
    writer_app_id: u64,
    writer_bot_login: &str,
) -> Result<bool, OrchestratorError> {
    let Some(review) = value
        .get("required_pull_request_reviews")
        .filter(|review| !review.is_null())
    else {
        return Ok(false);
    };
    let allowances = review
        .get("bypass_pull_request_allowances")
        .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
    let apps = allowances["apps"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
    let users = allowances["users"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
    let teams = allowances["teams"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
    let app_bypass = apps.iter().try_fold(false, |found, app| {
        let id = app["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
        Ok::<_, OrchestratorError>(found || id == writer_app_id)
    })?;
    let user_bypass = users.iter().try_fold(false, |found, user| {
        let login = user["login"]
            .as_str()
            .filter(|login| !login.is_empty())
            .ok_or_else(|| internal("tap_merge_classic_bypass_unreadable"))?;
        Ok::<_, OrchestratorError>(found || login.eq_ignore_ascii_case(writer_bot_login))
    })?;
    Ok(app_bypass || user_bypass || !teams.is_empty())
}
