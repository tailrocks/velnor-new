//! Keep one MBX cache owner for each runner lane.
//!
//! The pinned v1.6 action uses its native GitHub backend on hosted runners.
//! Scale Set uses the action's local backend and Velnor's manual bundle route.
//! The manual route stores one transport directory outside the MBX store and
//! keeps both until the disposable runner removes its temporary directory.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{MBX_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES, is_mbx_action};
use crate::matrix::MATRIX_NEEDS_JOB_ENV;
#[path = "mbx_bundle_import.rs"]
mod importer;
#[path = "mbx_bundle_lane.rs"]
mod lane;
#[path = "mbx_bundle_store.rs"]
mod store;
use importer::IMPORT_SCRIPT;
use lane::{
    configure_action_transport, insert_private_store_init, scope_bundle_route_to_scale_set,
};
use store::EXPORT_SCRIPT;

/// Display name of the reclaim-and-export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the one-file cache save.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the prefix step ahead of the bundle restore.
pub(crate) const MBX_BUNDLE_KEY_NAME: &str = "Prepare MBX bundle key";
/// Display name of the bundle restore. Same path the save archived.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import into the mbx store.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Display name of the fresh per-job MBX store setup.
pub(crate) const MBX_STORE_INIT_NAME: &str = "Initialize private MBX store";
/// One transport path shared by restore and save to preserve cache versions.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";

/// Locate an exact renderer-owned MBX script in a named shell step.
///
/// This is the only multiline-script path. The script body must match one
/// fixed MBX script exactly, before or after the renderer's credential prelude.
pub(crate) fn trusted_script_argument(name: &str, argv: &[String]) -> Option<usize> {
    let expected = match name {
        MBX_STORE_INIT_NAME => store::STORE_INIT_SCRIPT,
        MBX_BUNDLE_KEY_NAME => KEY_SCRIPT,
        MBX_BUNDLE_IMPORT_NAME => IMPORT_SCRIPT,
        MBX_BUNDLE_EXPORT_NAME => EXPORT_SCRIPT,
        _ => return None,
    };
    let prefix = crate::toolchain_env::unset_prefix_len(argv);
    if prefix > 0 {
        let expected_prefix = crate::toolchain_env::with_env_unset_argv(&[]);
        if prefix != expected_prefix.len() || argv.get(..prefix) != Some(expected_prefix.as_slice())
        {
            return None;
        }
    }
    let shell = argv.get(prefix..)?;
    if shell.len() != 3 || shell[0] != "bash" || shell[1] != "-c" {
        return None;
    }
    let script = &shell[2];
    let credential_prefixed = crate::toolchain_env::with_credential_unset_script(expected);
    (script == expected || script == &credential_prefixed).then_some(prefix + 2)
}

/// Recognize only the one complete renderer-owned MBX export step in a job.
pub(crate) fn is_exact_generated_export_step(job: &Job, candidate: &Step) -> bool {
    let Ok(expected) = export_step() else {
        return false;
    };
    if candidate != &expected {
        return false;
    }
    let mut exports = job
        .steps
        .iter()
        .filter(|step| step.name == MBX_BUNDLE_EXPORT_NAME);
    exports.next().is_some_and(|step| step == candidate) && exports.next().is_none()
}

/// Push-only export gate. A failed import handoff cannot publish a bundle.
const PREP_IF: &str = "success() && github.event_name == 'push' && steps.mbx-import.outcome == 'success' && (steps.mbx-import.outputs.cache-state == 'cold' || steps.mbx-import.outputs.cache-state == 'imported') && steps.mbx-bundle.outputs.cache-hit != 'true'";
/// Save gate. Empty exports set `ready=false` and must not call `actions/cache`.
const SAVE_IF: &str = "success() && github.event_name == 'push' && steps.mbx-import.outcome == 'success' && (steps.mbx-import.outputs.cache-state == 'cold' || steps.mbx-import.outputs.cache-state == 'imported') && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outcome == 'success' && steps.mbx-export.outputs.ready == 'true'";
/// Require a prepared private store before the manual Scale Set cache route.
const STORE_READY_IF: &str = "success() && steps.mbx-store-init.outcome == 'success' && steps.mbx-store-init.outputs.ready == 'true'";
/// Restore by stable writer identity before the compatible common prefix.
const KEY_SCRIPT: &str = r#"set -eu
cache_unavailable() {
    reason="$1"
    if [ -z "${GITHUB_OUTPUT:-}" ] || ! printf \
        'ready=false\nacceptance=cache_unavailable\n' >> "$GITHUB_OUTPUT" 2>/dev/null; then
        printf '::warning::MBX cache key output handoff failed.\n' >&2
        exit 1
    fi
    printf '::warning::MBX cache key unavailable: %s\n' "$reason" >&2
    exit 0
}

job_id=${MBX_JOB_ID:-}
matrix_key=${MBX_MATRIX_KEY:-}
run_id=${MBX_RUN_ID:-}
run_attempt=${MBX_RUN_ATTEMPT:-}
case "$job_id" in ''|*[!A-Za-z0-9_-]*) cache_unavailable "invalid-job-identity" ;; esac
case "$run_id" in ''|*[!0-9]*) cache_unavailable "invalid-run-identity" ;; esac
case "$run_attempt" in ''|*[!0-9]*) cache_unavailable "invalid-run-identity" ;; esac
case "${RUNNER_OS:-}" in
    Linux) os=linux ;;
    macOS) os=darwin ;;
    Windows) os=win32 ;;
    *) cache_unavailable "unsupported-runner-os" ;;
esac
case "${RUNNER_ARCH:-}" in
    X64) arch=x64 ;;
    ARM64) arch=arm64 ;;
    *) cache_unavailable "unsupported-runner-arch" ;;
esac
generation=${MBX_GENERATION:-}
toolchain=${MBX_TOOLCHAIN:-}
case "$generation" in ''|*[!A-Za-z0-9._-]*) cache_unavailable "invalid-cache-generation" ;; esac
case "$toolchain" in ''|*[!A-Za-z0-9._-]*) cache_unavailable "invalid-toolchain" ;; esac
if ! rustc_output=$(rustc "+$toolchain" -vV); then
    cache_unavailable "rustc-identity-unavailable"
fi
if ! rustc_hash=$(printf '%s' "$rustc_output" | sha256sum | cut -c1-12); then
    cache_unavailable "rustc-identity-hash-failed"
fi
case "$rustc_hash" in ????????????) ;; *) cache_unavailable "bad-rustc-identity-hash" ;; esac
compatibility_key="${os}-${arch}-mbx-${generation}-rust-${rustc_hash}"
case "$compatibility_key" in *[!A-Za-z0-9._-]*) cache_unavailable "invalid-compatibility-key" ;; esac
case "$matrix_key" in
    '') writer_identity="j${#job_id}-${job_id}-n" ;;
    m-????????????????)
        matrix_digest=${matrix_key#m-}
        case "$matrix_digest" in *[!a-f0-9]*) cache_unavailable "invalid-matrix-identity" ;; esac
        writer_identity="j${#job_id}-${job_id}-m-${matrix_digest}"
        ;;
    *) cache_unavailable "invalid-matrix-identity" ;;
esac
writer_prefix="${compatibility_key}-${writer_identity}-"
primary="${writer_prefix}r${run_id}-a${run_attempt}"
if [ "${#primary}" -gt 512 ] || [ -z "${GITHUB_OUTPUT:-}" ]; then
    cache_unavailable "cache-key-output-unavailable"
fi
fallback="${compatibility_key}-"
if ! printf 'ready=true\nkey=%s\nprefix=%s\nfallback=%s\n' \
    "$primary" "$writer_prefix" "$fallback" >> "$GITHUB_OUTPUT"; then
    printf '::warning::MBX cache key output handoff failed.\n' >&2
    exit 1
fi"#;
/// YAML step id for the MBX restore and export steps, when they have one.
pub(crate) fn step_yaml_id(name: &str) -> Option<&'static str> {
    match name {
        MBX_RESTORE_NAME => Some("mbx"),
        MBX_STORE_INIT_NAME => Some("mbx-store-init"),
        MBX_BUNDLE_KEY_NAME => Some("mbx-bundle-key"),
        MBX_BUNDLE_RESTORE_NAME => Some("mbx-bundle"),
        MBX_BUNDLE_IMPORT_NAME => Some("mbx-import"),
        MBX_BUNDLE_EXPORT_NAME => Some("mbx-export"),
        _ => None,
    }
}

/// Emit `id:` for MBX steps whose later steps read outputs.
pub(crate) fn push_step_id(entries: &mut Vec<(String, crate::yaml::Yaml)>, name: &str) {
    let Some(id) = step_yaml_id(name) else {
        return;
    };
    entries.push(("id".to_owned(), crate::yaml::Yaml::str(id.to_owned())));
}

/// Force every MBX action to restore-only and append the single-bundle save.
///
/// # Errors
///
/// Returns [`RenderError`] when the export or save step fails validation.
pub(crate) fn append_single_bundle_saves(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    for (job_id, job) in jobs.iter_mut() {
        if !job.steps.iter().any(is_mbx_action) {
            continue;
        }
        configure_action_transport(&mut job.steps);
        let matrix_job = is_matrix_job(job_id, job);
        insert_private_store_init(job, matrix_job)?;
        insert_bundle_restore(job, matrix_job)?;
        if !job
            .steps
            .iter()
            .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME)
        {
            job.steps.push(export_step()?);
            job.steps.push(save_step()?);
        }
        scope_bundle_route_to_scale_set(job);
    }
    Ok(())
}

fn is_matrix_job(job_id: &str, job: &Job) -> bool {
    job_id == crate::render::TASK_JOB_ID
        && job.steps.iter().any(|step| match &step.kind {
            StepKind::Shell { env, .. } => env.contains_key(MATRIX_NEEDS_JOB_ENV),
            _ => false,
        })
}

fn insert_bundle_restore(job: &mut Job, matrix_job: bool) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| step.name == MBX_BUNDLE_RESTORE_NAME)
    {
        return Ok(());
    }
    let Some(index) = job.steps.iter().position(is_mbx_action) else {
        return Ok(());
    };
    let at = index + 1;
    let added = [
        key_step(matrix_job, &job.steps[index])?,
        restore_step()?,
        import_step()?,
    ];
    job.steps.splice(at..at, added);
    Ok(())
}

fn key_step(matrix_job: bool, action: &Step) -> Result<Step, RenderError> {
    let StepKind::Action { with, .. } = &action.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_key_without_action".to_owned(),
        ));
    };
    let generation = with
        .get("cache-generation")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_cache_generation_missing".to_owned()))?;
    let generation = lane::directory_cache_generation(generation)?;
    let toolchain = with
        .get("toolchain")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_toolchain_missing".to_owned()))?;
    let mut env = BTreeMap::from([
        ("MBX_JOB_ID".to_owned(), "${{ github.job }}".to_owned()),
        ("MBX_RUN_ID".to_owned(), "${{ github.run_id }}".to_owned()),
        (
            "MBX_RUN_ATTEMPT".to_owned(),
            "${{ github.run_attempt }}".to_owned(),
        ),
        ("MBX_GENERATION".to_owned(), generation),
        ("MBX_TOOLCHAIN".to_owned(), toolchain.clone()),
    ]);
    if matrix_job {
        env.insert(
            "MBX_MATRIX_KEY".to_owned(),
            "${{ matrix.matrix_key }}".to_owned(),
        );
    } else {
        env.insert("MBX_MATRIX_KEY".to_owned(), String::new());
    }
    crate::steps::shell_step(
        MBX_BUNDLE_KEY_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), KEY_SCRIPT.to_owned()],
        env,
    )
}

fn restore_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.key }}".to_owned(),
            ),
            (
                "restore-keys".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.prefix }}\n${{ steps.mbx-bundle-key.outputs.fallback }}"
                    .to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(
        "success() && steps.mbx-bundle-key.outcome == 'success' && steps.mbx-bundle-key.outputs.ready == 'true'"
            .to_owned(),
    );
    Ok(step)
}

fn import_step() -> Result<Step, RenderError> {
    let env = BTreeMap::from([(
        "MATCHED".to_owned(),
        "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
    )]);
    crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), IMPORT_SCRIPT.to_owned()],
        env,
    )
    .map(|mut step| {
        step.condition = Some(
            "success() && steps.mbx-bundle-key.outcome == 'success' && steps.mbx-bundle-key.outputs.ready == 'true' && steps.mbx-bundle.outcome == 'success'"
                .to_owned(),
        );
        step
    })
}

fn export_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), EXPORT_SCRIPT.to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(PREP_IF.to_owned());
    Ok(step)
}

fn save_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_SAVE_NAME,
        TOOLS_SAVE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.key }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(SAVE_IF.to_owned());
    Ok(step)
}

#[cfg(test)]
#[path = "mbx_bundle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mbx_store_tests.rs"]
mod store_tests;

#[cfg(test)]
#[path = "mbx_store_policy_tests.rs"]
mod store_policy_tests;
