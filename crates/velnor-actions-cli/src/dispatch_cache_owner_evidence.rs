//! Record and combine physical qualification cache owner evidence.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation, QualificationCacheLayer,
    QualificationCacheLayerReceipt, QualificationCacheRestore, QualificationCacheRestoreResult,
    QualificationCacheSave, QualificationCacheSaveActionResult,
};
use velnor_actions_orchestrator::{
    QualificationCacheBindingRequest, aggregate_qualification_cache_owner_evidence,
    bind_qualification_cache_runtime, qualification_cache_runtime_after_import_filename,
    run_key_for_ci, write_qualification_cache_owner_layer_evidence,
};

use super::{cache_bind, fail_internal};

pub(crate) const LAYER_OP: &str = "record-qualification-cache-owner-layer-v1";
pub(crate) const OWNER_OP: &str = "aggregate-qualification-cache-owner-v1";
const OWNER_ID: &str = "VELNOR_QUALIFICATION_CACHE_OWNER_ID";
const LAYER: &str = "VELNOR_QUALIFICATION_CACHE_LAYER";
const RUNTIME_PHASE: &str = "VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE";
const AFTER_RUNTIME_FILE: &str = "VELNOR_QUALIFICATION_CACHE_RUNTIME_AFTER_FILE";
const CENSUS_PREFIX: &str = "VELNOR_QUALIFICATION_CACHE_";
const EXPORT_PREFIX: &str = "VELNOR_QUALIFICATION_CACHE_EXPORT_";
const SAVE_ACTION: &str = "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION";

pub(crate) fn record_layer() -> ExitCode {
    match record_layer_inner() {
        Ok(path) => write_output("owner_layer_evidence_path", &path),
        Err(problem) => fail_internal(problem),
    }
}

pub(crate) fn aggregate_owner() -> ExitCode {
    match aggregate_owner_inner() {
        Ok(path) => write_output("owner_evidence_path", &path),
        Err(problem) => fail_internal(problem),
    }
}

fn record_layer_inner() -> Result<String, &'static str> {
    let inputs = read_inputs_after_import()?;
    let request = inputs.request();
    let owner_id = required(OWNER_ID)?;
    let layer_text = required(LAYER)?;
    let layer = parse_layer(&layer_text)?;
    let context = velnor_actions_orchestrator::qualification_cache_owner_layer_context(&request)
        .map_err(|_| "qualification_cache_owner_layer_invalid")?;
    if context.owner.owner_id != owner_id || context.layer.layer != layer {
        return Err("qualification_cache_owner_layer_mismatch");
    }

    let receipt = if context.runtime_identity.is_some() {
        active_layer_receipt(&request, &context, &owner_id, &layer_text)?
    } else {
        QualificationCacheLayerReceipt {
            owner_id,
            platform_id: context.owner.platform_id,
            member_digest: context.owner.member_digest,
            layer,
            active: false,
            identity_digest: context.layer.identity_digest,
            runtime_identity: None,
            state_digest: None,
            restore: QualificationCacheRestore {
                requested_key: None,
                matched_key: None,
                matched_cache: QualificationCacheBackendObservation::NotQueried,
                result: QualificationCacheRestoreResult::Disabled,
            },
            save: QualificationCacheSave {
                requested_key: None,
                action: QualificationCacheSaveActionResult::Disabled,
                before: QualificationCacheBackendObservation::NotQueried,
                after: QualificationCacheBackendObservation::NotQueried,
            },
        }
    };
    write_qualification_cache_owner_layer_evidence(&request, &receipt)
        .map_err(|_| "qualification_cache_owner_layer_write_failed")
}

fn active_layer_receipt(
    request: &QualificationCacheBindingRequest<'_>,
    context: &velnor_actions_orchestrator::QualificationCacheOwnerLayerContext,
    owner_id: &str,
    layer_text: &str,
) -> Result<QualificationCacheLayerReceipt, &'static str> {
    let after_name = qualification_cache_runtime_after_import_filename(owner_id, layer_text)
        .map_err(|_| "qualification_cache_runtime_after_import_invalid")?;
    let run_key = run_key_for_ci(request.run_id, u64::from(request.run_attempt));
    let expected_after_path = request
        .runner_temp
        .join("velnor")
        .join(&run_key)
        .join(after_name);
    let supplied_after_path = PathBuf::from(required(AFTER_RUNTIME_FILE)?);
    if supplied_after_path != expected_after_path {
        return Err("qualification_cache_runtime_after_import_path_mismatch");
    }
    let binding = bind_qualification_cache_runtime(request)
        .map_err(|_| "qualification_cache_binding_invalid")?;
    let restore = restore_observation(binding.restore_key.as_ref())?;
    let export_result = required(&format!("{EXPORT_PREFIX}EXPORT_RESULT"))?;
    let save_requested = bool_value(&format!("{EXPORT_PREFIX}SAVE_REQUESTED"))?;
    let state_digest = required(&format!("{EXPORT_PREFIX}STATE_DIGEST"))?;
    let (save_before, save_after, save_action) = if save_requested {
        (
            Some(census_observation(
                "SAVE_BEFORE",
                binding.save_key.as_deref(),
            )?),
            Some(census_observation(
                "SAVE_AFTER",
                binding.save_key.as_deref(),
            )?),
            env::var(SAVE_ACTION).ok().filter(|value| !value.is_empty()),
        )
    } else {
        (
            None,
            None,
            env::var(SAVE_ACTION).ok().filter(|value| !value.is_empty()),
        )
    };
    let save = save_evidence(
        binding.save_key,
        &export_result,
        save_requested,
        save_before,
        save_after,
        save_action.as_deref(),
    )?;
    Ok(QualificationCacheLayerReceipt {
        owner_id: owner_id.to_owned(),
        platform_id: context.owner.platform_id.clone(),
        member_digest: context.owner.member_digest.clone(),
        layer: context.layer.layer,
        active: true,
        identity_digest: context.layer.identity_digest.clone(),
        runtime_identity: context.runtime_identity.clone(),
        state_digest: Some(state_digest),
        restore,
        save,
    })
}

fn aggregate_owner_inner() -> Result<String, &'static str> {
    let inputs = read_inputs_before_import()?;
    let paths = [
        "MBX_OBJECTS",
        "CARGO_SOURCES",
        "MISE_TOOLS",
        "TOFU_PROVIDERS",
        "TASK_RESULT",
    ]
    .map(|layer| {
        required(&format!(
            "VELNOR_QUALIFICATION_CACHE_LAYER_{layer}_EVIDENCE"
        ))
    })
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .map(PathBuf::from)
    .collect::<Vec<_>>();
    aggregate_qualification_cache_owner_evidence(&inputs.request(), &paths)
        .map_err(|_| "qualification_cache_owner_aggregate_failed")
}

fn read_inputs_before_import() -> Result<super::cache_bind::BindInputs, &'static str> {
    cache_bind::read_inputs(|name| {
        if name == RUNTIME_PHASE {
            return Ok("before_import".to_owned());
        }
        env::var(name)
    })
    .map_err(|_| "qualification_cache_binding_inputs_invalid")
}

fn read_inputs_after_import() -> Result<super::cache_bind::BindInputs, &'static str> {
    cache_bind::read_inputs(|name| env::var(name))
        .map_err(|_| "qualification_cache_binding_inputs_invalid")
}

fn restore_observation(
    expected_key: Option<&String>,
) -> Result<QualificationCacheRestore, &'static str> {
    let observed = census_observation("RESTORE", expected_key.map(String::as_str))?;
    let prefix = format!("{CENSUS_PREFIX}RESTORE_CENSUS_");
    let hit = bool_value(&format!("{prefix}ACTION_HIT"))?;
    let matched_key = optional(&format!("{prefix}ACTION_MATCHED_KEY"));
    if hit != matched_key.is_some()
        || hit != matches!(observed, QualificationCacheBackendObservation::Found(_))
    {
        return Err("qualification_cache_restore_observation_mismatch");
    }
    Ok(QualificationCacheRestore {
        requested_key: expected_key.cloned(),
        matched_key,
        matched_cache: observed,
        result: if hit {
            QualificationCacheRestoreResult::Hit
        } else {
            QualificationCacheRestoreResult::Miss
        },
    })
}

fn census_observation(
    phase: &str,
    expected_key: Option<&str>,
) -> Result<QualificationCacheBackendObservation, &'static str> {
    let prefix = format!("{CENSUS_PREFIX}{phase}_CENSUS_");
    if required(&format!("{prefix}PHASE"))? != phase.to_ascii_lowercase()
        || optional(&format!("{prefix}KEY")).as_deref() != expected_key
        || required(&format!("{prefix}ADMITTED"))? != "true"
    {
        return Err("qualification_cache_census_mismatch");
    }
    match required(&format!("{prefix}STATUS"))?.as_str() {
        "absent" if expected_key.is_some() => {
            ensure_empty_cache(&prefix)?;
            Ok(QualificationCacheBackendObservation::Absent)
        }
        "not_queried" if expected_key.is_none() => {
            ensure_empty_cache(&prefix)?;
            Ok(QualificationCacheBackendObservation::NotQueried)
        }
        "found" if expected_key.is_some() => {
            let entry = QualificationCacheBackendEntry {
                id: positive_decimal(&format!("{prefix}CACHE_ID"))?,
                key: required(&format!("{prefix}CACHE_KEY"))?,
                git_ref: required(&format!("{prefix}CACHE_REF"))?,
                version: required(&format!("{prefix}CACHE_VERSION"))?,
                size_bytes: positive_decimal(&format!("{prefix}CACHE_SIZE"))?,
            };
            if Some(entry.key.as_str()) != expected_key {
                return Err("qualification_cache_census_key_mismatch");
            }
            Ok(QualificationCacheBackendObservation::Found(entry))
        }
        _ => Err("qualification_cache_census_status_invalid"),
    }
}

fn save_observation(
    requested_key: Option<String>,
    export_result: &str,
    save_requested: bool,
    before: QualificationCacheBackendObservation,
    after: QualificationCacheBackendObservation,
    action: Option<&str>,
) -> Result<QualificationCacheSave, &'static str> {
    let action = match (
        requested_key.is_some(),
        export_result,
        save_requested,
        action,
    ) {
        (false, "not_required", false, _) => QualificationCacheSaveActionResult::Disabled,
        (true, "not_required", false, _) => QualificationCacheSaveActionResult::NotRequired,
        (true, "staged", true, Some("success")) => QualificationCacheSaveActionResult::Succeeded,
        (true, "staged", true, Some("skipped")) => QualificationCacheSaveActionResult::Skipped,
        (true, "staged", true, Some("failure" | "cancelled" | "timed_out")) => {
            QualificationCacheSaveActionResult::Failed
        }
        _ => return Err("qualification_cache_save_action_invalid"),
    };
    Ok(QualificationCacheSave {
        requested_key: if action == QualificationCacheSaveActionResult::NotRequired {
            None
        } else {
            requested_key
        },
        action,
        before,
        after,
    })
}

fn save_evidence(
    requested_key: Option<String>,
    export_result: &str,
    save_requested: bool,
    before: Option<QualificationCacheBackendObservation>,
    after: Option<QualificationCacheBackendObservation>,
    action: Option<&str>,
) -> Result<QualificationCacheSave, &'static str> {
    if !save_requested {
        if export_result != "not_required"
            || action.is_some()
            || before.is_some()
            || after.is_some()
        {
            return Err("qualification_cache_unscheduled_save_has_observations");
        }
        return save_observation(
            requested_key,
            export_result,
            false,
            QualificationCacheBackendObservation::NotQueried,
            QualificationCacheBackendObservation::NotQueried,
            None,
        );
    }
    if export_result != "staged" {
        return Err("qualification_cache_save_stage_missing");
    }
    save_observation(
        requested_key,
        export_result,
        true,
        before.ok_or("qualification_cache_save_before_census_missing")?,
        after.ok_or("qualification_cache_save_after_census_missing")?,
        action,
    )
}

fn parse_layer(value: &str) -> Result<QualificationCacheLayer, &'static str> {
    match value {
        "mbx_objects" => Ok(QualificationCacheLayer::MbxObjects),
        "cargo_sources" => Ok(QualificationCacheLayer::CargoSources),
        "mise_tools" => Ok(QualificationCacheLayer::MiseTools),
        "tofu_providers" => Ok(QualificationCacheLayer::TofuProviders),
        "task_result" => Ok(QualificationCacheLayer::TaskResult),
        _ => Err("qualification_cache_layer_invalid"),
    }
}

fn required(name: &str) -> Result<String, &'static str> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty() && value.len() <= 4_096 && value.is_ascii())
        .ok_or("qualification_cache_observation_missing")
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn bool_value(name: &str) -> Result<bool, &'static str> {
    match required(name)?.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err("qualification_cache_boolean_invalid"),
    }
}

fn positive_decimal(name: &str) -> Result<u64, &'static str> {
    let text = required(name)?;
    let value = text
        .parse::<u64>()
        .map_err(|_| "qualification_cache_decimal_invalid")?;
    if value == 0 || value.to_string() != text {
        return Err("qualification_cache_decimal_invalid");
    }
    Ok(value)
}

fn ensure_empty_cache(prefix: &str) -> Result<(), &'static str> {
    for suffix in [
        "CACHE_ID",
        "CACHE_KEY",
        "CACHE_REF",
        "CACHE_VERSION",
        "CACHE_SIZE",
    ] {
        if optional(&format!("{prefix}{suffix}")).is_some() {
            return Err("qualification_cache_census_unexpected_entry");
        }
    }
    Ok(())
}

fn write_output(name: &str, value: &str) -> ExitCode {
    if value.len() > 4_096 || value.bytes().any(|byte| matches!(byte, b'\n' | b'\r' | 0)) {
        return fail_internal("qualification_cache_evidence_output_invalid");
    }
    let Some(path) = env::var_os(cache_bind::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty())
    else {
        return fail_internal("missing_github_output");
    };
    let body = format!("{name}={value}\n");
    match cache_bind::append_owned_output(&path, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => fail_internal("qualification_cache_evidence_output_failed"),
    }
}

#[cfg(test)]
#[path = "dispatch_cache_owner_evidence_tests.rs"]
mod tests;
