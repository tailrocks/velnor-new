//! Opaque MBX import/export step construction and render-time validation.

use std::collections::BTreeMap;

use velnor_actions_contract::{MbxProcessGroupOperation, MbxProcessGroupShell, Step, StepKind};

use crate::{RenderError, commands, toolchain_env};

const EXPORT_CONDITION: &str = "(success() && github.event_name == 'push' && toJSON(github.event.repository.fork) == 'false' && github.event.repository.default_branch != '' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true) && steps.mbx_cache_import.outputs.store_ready == 'true' && steps.mbx_cache_import.outputs.export_allowed == 'true'";
const FIXED_SUBSTITUTIONS: [&str; 16] = [
    "$(ps -A -o pid= -o pgid= -o stat=)",
    "$(stat -f '%d' \"$root\")",
    "$(stat -c '%d' -- \"$root\")",
    "$(find -P \"$root\" -xdev ! -type d ! -type f -print -quit)",
    "$(stat -f \"%d\" \"$path\")",
    "$(stat -c \"%d\" -- \"$path\")",
    "$(stat -f \"%d:%l\" \"$path\")",
    "$(stat -c \"%d:%h\" -- \"$path\")",
    "$(stat -f \"%d\" \"$anchor\")",
    "$(stat -c \"%d\" -- \"$anchor\")",
    "$(stat -f \"%d\" \"$target\")",
    "$(stat -c \"%d\" -- \"$target\")",
    "$(find -P \"$tree\" -xdev -type f -links 1 -print -quit)",
    "$(stat -f '%d:%i' \"$bundle\")",
    "$(stat -c '%d:%i' -- \"$bundle\")",
    "$((attempt + 1))",
];

const PREFLIGHT_ENV: [(&str, &str); 17] = [
    (
        "VELNOR_MBX_JOB_ROOT",
        "${{ steps.mbx_cache_preflight.outputs.job_root }}",
    ),
    (
        "VELNOR_MBX_JOB_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.job_identity }}",
    ),
    (
        "VELNOR_MBX_JOB_ROOT_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.job_root_identity }}",
    ),
    (
        "VELNOR_MBX_WORKSPACE_ROOT",
        "${{ steps.mbx_cache_preflight.outputs.workspace_root }}",
    ),
    (
        "VELNOR_MBX_WORKSPACE_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.workspace_identity }}",
    ),
    (
        "VELNOR_MBX_TRANSPORT_PARENT",
        "${{ steps.mbx_cache_preflight.outputs.transport_parent }}",
    ),
    (
        "VELNOR_MBX_TRANSPORT_PARENT_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.transport_parent_identity }}",
    ),
    (
        "VELNOR_MBX_TRANSPORT_LEASE",
        "${{ steps.mbx_cache_preflight.outputs.transport_lease }}",
    ),
    (
        "VELNOR_MBX_TRANSPORT_LEASE_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.transport_lease_identity }}",
    ),
    (
        "VELNOR_MBX_PRIMARY_CACHE_DIR",
        "${{ steps.mbx_cache_preflight.outputs.cache_dir }}",
    ),
    (
        "VELNOR_MBX_PRIMARY_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.cache_dir_identity }}",
    ),
    (
        "VELNOR_MBX_COLD_CACHE_DIR",
        "${{ steps.mbx_cache_preflight.outputs.cold_cache_dir }}",
    ),
    (
        "VELNOR_MBX_COLD_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.cold_cache_dir_identity }}",
    ),
    (
        "VELNOR_MBX_BUNDLE_PATH",
        "${{ steps.mbx_cache_preflight.outputs.bundle_path }}",
    ),
    (
        "VELNOR_MBX_BUNDLE_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.bundle_path_identity }}",
    ),
    (
        "VELNOR_MBX_IMPORT_CWD",
        "${{ steps.mbx_cache_preflight.outputs.import_cwd }}",
    ),
    (
        "VELNOR_MBX_IMPORT_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.import_cwd_identity }}",
    ),
];

/// Construct one fixed stock-MBX process-group operation.
///
/// There is deliberately no script argument. The renderer chooses the
/// canonical body from the operation tag during YAML serialization.
/// # Errors
pub(crate) fn mbx_process_group_shell_step(
    operation: MbxProcessGroupOperation,
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    let expected = expected_base_env(operation);
    if env != &expected {
        return Err(RenderError::InvalidWorkflow(
            "mbx_process_group_env_mismatch".to_owned(),
        ));
    }
    commands::validate_env(env)?;
    toolchain_env::reject_denied_step_keys(env)?;
    let (name, condition, payload) = match operation {
        MbxProcessGroupOperation::Import => (
            crate::mbx_bundle::MBX_BUNDLE_IMPORT_NAME,
            None,
            MbxProcessGroupShell::import(toolchain_env::with_credential_scrub(env)),
        ),
        MbxProcessGroupOperation::Export => (
            crate::mbx_bundle::MBX_BUNDLE_EXPORT_NAME,
            Some(EXPORT_CONDITION.to_owned()),
            MbxProcessGroupShell::export(toolchain_env::with_credential_scrub(env)),
        ),
    };
    Ok(Step {
        name: name.to_owned(),
        condition,
        continue_on_error: None,
        kind: StepKind::MbxProcessGroupShell(payload),
    })
}

/// Revalidate the immutable payload plus mutable outer step fields.
/// # Errors
pub(crate) fn validate_step(
    step: &Step,
    payload: &MbxProcessGroupShell,
) -> Result<(), RenderError> {
    let (expected_name, expected_condition, operation) = match payload.operation() {
        MbxProcessGroupOperation::Import => (
            crate::mbx_bundle::MBX_BUNDLE_IMPORT_NAME,
            None,
            MbxProcessGroupOperation::Import,
        ),
        MbxProcessGroupOperation::Export => (
            crate::mbx_bundle::MBX_BUNDLE_EXPORT_NAME,
            Some(EXPORT_CONDITION),
            MbxProcessGroupOperation::Export,
        ),
    };
    if step.name != expected_name
        || step.condition.as_deref() != expected_condition
        || step.continue_on_error.is_some()
        || payload.env() != &toolchain_env::with_credential_scrub(&expected_base_env(operation))
    {
        return Err(RenderError::InvalidWorkflow(
            "mbx_process_group_step_mutated".to_owned(),
        ));
    }
    commands::validate_env(payload.env())?;
    Ok(())
}

/// Return the canonical Bash argv for an opaque MBX operation.
/// # Errors
pub(crate) fn rendered_run(
    step: &Step,
    payload: &MbxProcessGroupShell,
) -> Result<Vec<String>, RenderError> {
    validate_step(step, payload)?;
    let run = crate::stock_mbx_bundle::process_group_run(payload.operation());
    validate_fixed_script(payload.operation(), &run)?;
    Ok(run)
}

fn validate_fixed_script(
    operation: MbxProcessGroupOperation,
    run: &[String],
) -> Result<(), RenderError> {
    if run.len() != 3 || run[0] != "bash" || run[1] != "-c" {
        return Err(RenderError::InvalidWorkflow(
            "mbx_process_group_argv_changed".to_owned(),
        ));
    }
    let script = &run[2];
    let launch = "mbx \"$@\" & mbx_pid=$!";
    let operation_call = match operation {
        MbxProcessGroupOperation::Import => "run_mbx cache import \"$bundle\"",
        MbxProcessGroupOperation::Export => {
            "run_mbx cache export --group \"$group\" --format directory \"$bundle\""
        }
    };
    let Some(start) = script.find("start_mbx() {") else {
        return invalid_fixed_script();
    };
    let Some(launch_at) = script.find(launch) else {
        return invalid_fixed_script();
    };
    let launching_marker = "mbx_launching=true\n";
    let launching_at = script.find(launching_marker);
    let Some(operation_at) = script.find(operation_call) else {
        return invalid_fixed_script();
    };
    if script.matches(launch).count() != 1
        || script.matches(operation_call).count() != 1
        || script.matches(launching_marker).count() != 1
        || !script.contains("set -m")
        || script.find("set -m").is_none_or(|at| at >= start)
        || launching_at.is_none_or(|at| at < start || at >= launch_at)
        || launch_at < start
        || operation_at < start
    {
        return invalid_fixed_script();
    }
    let validated_script =
        validate_fixed_substitutions(script)?.replace(launch, "mbx \"$@\" mbx_pid=$!");
    // Generic argv rejects control characters; validate this fixed multiline
    // body as a single-line copy without changing the emitted script.
    let sanitized = validated_script.replace('\n', "; ");
    commands::validate_command_argv(&["bash".to_owned(), "-c".to_owned(), sanitized])?;
    crate::steps::scan_for_private_subcommands(script)
}

fn validate_fixed_substitutions(script: &str) -> Result<String, RenderError> {
    let mut sanitized = script.to_owned();
    for substitution in FIXED_SUBSTITUTIONS {
        sanitized = sanitized.replace(substitution, "fixed_substitution");
    }
    if sanitized.contains("$(") {
        return invalid_fixed_script();
    }
    Ok(sanitized)
}

fn invalid_fixed_script<T>() -> Result<T, RenderError> {
    Err(RenderError::InvalidWorkflow(
        "mbx_process_group_script_changed".to_owned(),
    ))
}

fn expected_base_env(operation: MbxProcessGroupOperation) -> BTreeMap<String, String> {
    let mut env: BTreeMap<_, _> = PREFLIGHT_ENV
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    match operation {
        MbxProcessGroupOperation::Import => {
            env.insert(
                "MBX_CACHE_DIR".to_owned(),
                "${{ steps.mbx_cache_preflight.outputs.cache_dir }}".to_owned(),
            );
            env.insert(
                "MBX_CACHE_EXPORT_GROUP".to_owned(),
                "${{ steps.mbx_cache_preflight.outputs.export_group }}".to_owned(),
            );
            env.insert(
                "MBX_CACHE_RESTORE_OUTCOME".to_owned(),
                "${{ steps.mbx_cache_restore.outcome }}".to_owned(),
            );
            env.insert(
                "MBX_CACHE_RESTORE_MATCHED_KEY".to_owned(),
                "${{ steps.mbx_cache_restore.outputs.cache-matched-key }}".to_owned(),
            );
        }
        MbxProcessGroupOperation::Export => {
            env.insert(
                "MBX_CACHE_DIR".to_owned(),
                "${{ steps.mbx_cache_import.outputs.cache_dir }}".to_owned(),
            );
            env.insert(
                "MBX_CACHE_EXPORT_GROUP".to_owned(),
                "${{ steps.mbx_cache_import.outputs.export_group }}".to_owned(),
            );
        }
    }
    env
}

#[cfg(test)]
#[path = "steps_mbx_process_group_tests.rs"]
mod tests;
