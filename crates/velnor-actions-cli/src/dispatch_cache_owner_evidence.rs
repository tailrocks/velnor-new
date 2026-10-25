//! Record and combine physical qualification cache owner evidence.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    QualificationCacheBindingRequest, aggregate_qualification_cache_worker_owner_facts,
    bind_qualification_cache_runtime, qualification_cache_runtime_after_import_filename,
    run_key_for_ci, write_qualification_cache_worker_layer_facts,
};
use velnor_actions_orchestrator::{
    QualificationCacheLayer, QualificationCacheRestoreResult, QualificationCacheSaveActionResult,
    QualificationCacheWorkerLayerFacts, QualificationCacheWorkerRestoreFacts,
    QualificationCacheWorkerSaveFacts,
};

use super::{cache_bind, fail_internal};

pub(crate) const LAYER_OP: &str = "record-qualification-cache-owner-layer-v1";
pub(crate) const OWNER_OP: &str = "aggregate-qualification-cache-owner-v1";
const OWNER_ID: &str = "VELNOR_QUALIFICATION_CACHE_OWNER_ID";
const LAYER: &str = "VELNOR_QUALIFICATION_CACHE_LAYER";
const RUNTIME_PHASE: &str = "VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE";
const AFTER_RUNTIME_FILE: &str = "VELNOR_QUALIFICATION_CACHE_RUNTIME_AFTER_FILE";
const EXPORT_PREFIX: &str = "VELNOR_QUALIFICATION_CACHE_EXPORT_";
const SAVE_ACTION: &str = "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION";
const RESTORE_RESULT: &str = "VELNOR_QUALIFICATION_CACHE_RESTORE_RESULT";
const RESTORE_ACTION_HIT: &str = "VELNOR_QUALIFICATION_CACHE_HIT";
const RESTORE_ACTION_MATCHED_KEY: &str = "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY";

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

    let facts = if context.runtime_identity.is_some() {
        active_layer_facts(&request, &context, &owner_id, &layer_text)?
    } else {
        QualificationCacheWorkerLayerFacts {
            owner_id,
            platform_id: context.owner.platform_id,
            member_digest: context.owner.member_digest,
            layer,
            active: false,
            identity_digest: context.layer.identity_digest,
            runtime_identity: None,
            state_digest: None,
            restore: QualificationCacheWorkerRestoreFacts {
                requested_key: None,
                matched_key: None,
                result: QualificationCacheRestoreResult::Disabled,
            },
            save: QualificationCacheWorkerSaveFacts {
                requested_key: None,
                action: QualificationCacheSaveActionResult::Disabled,
            },
        }
    };
    write_qualification_cache_worker_layer_facts(&request, &facts)
        .map_err(|_| "qualification_cache_owner_layer_write_failed")
}

fn active_layer_facts(
    request: &QualificationCacheBindingRequest<'_>,
    context: &velnor_actions_orchestrator::QualificationCacheOwnerLayerContext,
    owner_id: &str,
    layer_text: &str,
) -> Result<QualificationCacheWorkerLayerFacts, &'static str> {
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
    let restore = restore_facts(binding.restore_key.as_deref())?;
    let export_result = required(&format!("{EXPORT_PREFIX}EXPORT_RESULT"))?;
    let save_requested = bool_value(&format!("{EXPORT_PREFIX}SAVE_REQUESTED"))?;
    let state_digest = required(&format!("{EXPORT_PREFIX}STATE_DIGEST"))?;
    let save_action = env::var(SAVE_ACTION).ok().filter(|value| !value.is_empty());
    let save = save_facts(
        binding.save_key.as_deref(),
        &export_result,
        save_requested,
        save_action.as_deref(),
    )?;
    Ok(QualificationCacheWorkerLayerFacts {
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

fn restore_facts(
    expected_key: Option<&str>,
) -> Result<QualificationCacheWorkerRestoreFacts, &'static str> {
    let result = required(RESTORE_RESULT)?;
    let hit = bool_value(RESTORE_ACTION_HIT)?;
    let matched_key = optional(RESTORE_ACTION_MATCHED_KEY);
    restore_facts_from_outputs(expected_key, &result, hit, matched_key)
}

fn restore_facts_from_outputs(
    expected_key: Option<&str>,
    result_text: &str,
    hit: bool,
    matched_key: Option<String>,
) -> Result<QualificationCacheWorkerRestoreFacts, &'static str> {
    let result = match result_text {
        "disabled" => QualificationCacheRestoreResult::Disabled,
        "miss" => QualificationCacheRestoreResult::Miss,
        "hit" => QualificationCacheRestoreResult::Hit,
        _ => return Err("qualification_cache_restore_result_invalid"),
    };
    if hit != matched_key.is_some() || (result == QualificationCacheRestoreResult::Hit) != hit {
        return Err("qualification_cache_restore_action_mismatch");
    }
    if expected_key.is_none() && (hit || result != QualificationCacheRestoreResult::Disabled) {
        return Err("qualification_cache_disabled_restore_observed");
    }
    if hit && matched_key.as_deref() != expected_key {
        return Err("qualification_cache_restore_key_mismatch");
    }
    Ok(QualificationCacheWorkerRestoreFacts {
        requested_key: expected_key.map(str::to_owned),
        matched_key,
        result,
    })
}

fn save_facts(
    expected_key: Option<&str>,
    export_result: &str,
    save_requested: bool,
    action: Option<&str>,
) -> Result<QualificationCacheWorkerSaveFacts, &'static str> {
    let (requested_key, action) = match (expected_key, export_result, save_requested, action) {
        (None, "not_required", false, None) => (None, QualificationCacheSaveActionResult::Disabled),
        (Some(_), "not_required", false, None) => {
            (None, QualificationCacheSaveActionResult::NotRequired)
        }
        (Some(key), "staged", true, Some("success")) => (
            Some(key.to_owned()),
            QualificationCacheSaveActionResult::Succeeded,
        ),
        (Some(key), "staged", true, Some("skipped")) => (
            Some(key.to_owned()),
            QualificationCacheSaveActionResult::Skipped,
        ),
        (Some(key), "staged", true, Some("failure" | "cancelled" | "timed_out")) => (
            Some(key.to_owned()),
            QualificationCacheSaveActionResult::Failed,
        ),
        _ => return Err("qualification_cache_save_action_invalid"),
    };
    Ok(QualificationCacheWorkerSaveFacts {
        requested_key,
        action,
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
    aggregate_qualification_cache_worker_owner_facts(&inputs.request(), &paths)
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
#[path = "../tests/dispatch_cache_owner_evidence_tests.rs"]
mod tests;
