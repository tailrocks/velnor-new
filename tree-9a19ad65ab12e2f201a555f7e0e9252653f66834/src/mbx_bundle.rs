//! Explicit, job-private MBX directory-cache lifecycle.
//!
//! The official action runs with its supported `local` backend, so it only
//! installs the pinned MBX release. Velnor then restores/imports and exports/
//! saves one stable directory through pinned `actions/cache` steps. Each job
//! gets a fresh MBX root; failed imports abandon that root before compilation.
//! The official exporter replaces an existing bundle atomically, and MBX GC
//! reclaims only state under the fresh root. Export still copies objects, so
//! the workflow does not claim a lower peak disk use without hosted workload
//! measurements.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{MBX_ACTION_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES, is_mbx_action};
#[path = "mbx_bundle_identity.rs"]
mod identity;
use identity::{CacheIdentity, import_guard, plan_writers};

/// Display name of the fresh, private MBX root step.
pub(crate) const MBX_ROOT_NAME: &str = "Prepare private MBX store";
/// Display name of the reclaim-and-export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the one-file cache save.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the key step ahead of the bundle restore.
pub(crate) const MBX_BUNDLE_KEY_NAME: &str = "Prepare MBX bundle key";
/// Display name of the bundle restore. Same path the save archived.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import into the private MBX store.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Bundle path and cache action version stay stable across jobs and runs.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";
/// Internal-only MBX input that selects a shared logical cache scope.
const MBX_SCOPE_INPUT: &str = "velnor-cache-scope";
/// Internal-only MBX input that marks an intentional cache reader/writer.
const MBX_WRITER_INPUT: &str = "velnor-cache-writer";
/// Reserved namespace keeps qualification bundles apart from job caches.
const MBX_QUALIFICATION_SCOPE_PREFIX: &str = "qualification-mbx-v1/";
const SHARED_CACHE_HIT: &str = "steps.mbx-lane-cache.outputs.mbx-cache-hit";
const SHARED_CACHE_KEY: &str = "${{ steps.mbx-lane-cache.outputs.mbx-cache-key }}";

/// Export only from the elected writer on a default-branch push.
const PREP_IF: &str = "success() && github.event_name == 'push' && github.ref_name == github.event.repository.default_branch && steps.mbx-bundle.outputs.cache-hit != 'true'";
/// Exact hits are immutable; prefix restores can publish the current revision.
const SAVE_IF: &str = "success() && github.event_name == 'push' && github.ref_name == github.event.repository.default_branch && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'";

/// Create one exclusive MBX cache root before any MBX command runs.
const ROOT_SCRIPT: &str = r#"set -eu; test -n "$RUNNER_TEMP"; bundle="$RUNNER_TEMP/mbx-single-bundle"; if [ -e "$bundle" ] || [ -L "$bundle" ]; then echo "stable MBX bundle path already exists in runner.temp" >&2; exit 1; fi; root_file="$GITHUB_OUTPUT.mbx-root"; if [ -e "$root_file" ] || [ -L "$root_file" ]; then echo "MBX root marker already exists" >&2; exit 1; fi; mktemp -d "$RUNNER_TEMP/velnor-mbx-store.XXXXXXXXXX" > "$root_file"; IFS= read -r root < "$root_file"; root_id="${root##*/}"; group="velnor-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${root_id}"; printf 'MBX_CACHE_DIR=%s\nMBX_TARGET_ROOT=%s/targets\nMBX_SHIMS_DIR=%s/shims\nMBX_CACHE_EXPORT_GROUP=%s\n' "$root" "$root" "$root" "$group" >> "$GITHUB_ENV""#;

/// Build the same directory-payload key dimensions as the official action.
const KEY_SCRIPT: &str = r#"set -eu; set -o pipefail; case "$RUNNER_OS:$RUNNER_ARCH" in Linux:X64|Linux:x64|Linux:AMD64) os=linux; arch=x64 ;; Linux:ARM64|Linux:arm64) os=linux; arch=arm64 ;; *) echo "unsupported runner for MBX directory cache key" >&2; exit 1 ;; esac; test "$MBX_VERSION" = "$MBX_EXPECTED_VERSION"; test -n "$RUSTUP_TOOLCHAIN"; workflow_ref=${GITHUB_WORKFLOW_REF:?missing GITHUB_WORKFLOW_REF}; case "$workflow_ref" in *@refs/*) workflow_path=${workflow_ref%%@refs/*} ;; *) echo "invalid GITHUB_WORKFLOW_REF" >&2; exit 1 ;; esac; test -n "$workflow_path"; scope_file="$GITHUB_OUTPUT.mbx-scope"; identity_file="$GITHUB_OUTPUT.mbx-rustc"; hash_file="$GITHUB_OUTPUT.mbx-hash"; for file in "$scope_file" "$identity_file" "$hash_file"; do if [ -e "$file" ] || [ -L "$file" ]; then echo "MBX key marker already exists" >&2; exit 1; fi; done; printf '%s\n%s\n%s\n' "$workflow_path" "$MBX_CACHE_SCOPE" "$MBX_MATRIX_CONTEXT" > "$scope_file"; sha256sum "$scope_file" | cut -c1-64 > "$hash_file"; IFS= read -r scope_hash < "$hash_file"; test -n "$scope_hash"; mise --no-config --no-env --no-hooks exec "rust@$RUSTUP_TOOLCHAIN" -- rustc -vV > "$identity_file"; sha256sum "$identity_file" | cut -c1-64 > "$hash_file"; IFS= read -r compiler_hash < "$hash_file"; test -n "$compiler_hash"; toolchain="rust-${RUSTUP_TOOLCHAIN}-${compiler_hash}"; revision="$GITHUB_SHA"; if [ -n "$MBX_BASE_SHA" ]; then revision="$MBX_BASE_SHA"; fi; case "$revision" in ''|*[!0-9a-f]*) exit 1 ;; esac; test "${#revision}" -eq 40; prefix="${os}-${arch}-mbx-${MBX_GENERATION}-dir-${toolchain}-scope-${scope_hash}-";"#;
const QUALIFICATION_KEY_SCRIPT: &str = r#"run_id=${GITHUB_RUN_ID:?missing GITHUB_RUN_ID}; run_attempt=${GITHUB_RUN_ATTEMPT:?missing GITHUB_RUN_ATTEMPT}; case "$run_id" in ''|*[!0-9]*) echo "invalid GITHUB_RUN_ID" >&2; exit 1 ;; esac; case "$run_attempt" in ''|*[!0-9]*) echo "invalid GITHUB_RUN_ATTEMPT" >&2; exit 1 ;; esac; prefix="${prefix}run-${run_id}-attempt-${run_attempt}-"; printf 'primary=%s%s\nprefix=%s\n' "$prefix" "$revision" "$prefix" >> "$GITHUB_OUTPUT""#;
const KEY_OUTPUT_SCRIPT: &str =
    r#"printf 'primary=%s%s\nprefix=%s\n' "$prefix" "$revision" "$prefix" >> "$GITHUB_OUTPUT""#;

/// Export first, then ask MBX's lock-aware collector to reclaim its private root.
const EXPORT_SCRIPT: &str = r#"set -eu; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; bundle="$RUNNER_TEMP/mbx-single-bundle"; export_log="$GITHUB_OUTPUT.mbx-export"; gc_log="$GITHUB_OUTPUT.mbx-gc"; for file in "$export_log" "$gc_log"; do if [ -e "$file" ] || [ -L "$file" ]; then echo "MBX export marker already exists" >&2; exit 1; fi; done; if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$export_log" 2>&1; then test -d "$bundle"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; if mbx gc --max-size 0 --json >"$gc_log" 2>&1; then cat "$gc_log"; echo 'gc-succeeded=true' >> "$GITHUB_OUTPUT"; echo 'ready=true' >> "$GITHUB_OUTPUT"; else cat "$gc_log"; echo 'gc-succeeded=false' >> "$GITHUB_OUTPUT"; echo 'ready=false' >> "$GITHUB_OUTPUT"; fi; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; else if grep -Fq 'no completed mbx builds are recorded for export group' "$export_log"; then cat "$export_log"; echo 'ready=false' >> "$GITHUB_OUTPUT"; exit 0; fi; cat "$export_log"; exit 1; fi"#;

/// Failed imports never keep compiling against a partially published store.
const IMPORT_SCRIPT: &str = r#"set -eu; bundle="$RUNNER_TEMP/mbx-single-bundle"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; if [ -z "$MATCHED" ]; then echo 'no MBX bundle matched'; exit 0; fi; if [ ! -d "$bundle" ]; then echo 'matched MBX bundle is missing; the fresh store stays cold'; exit 0; fi; if mbx cache import "$bundle"; then echo 'MBX bundle imported'; else echo 'MBX bundle import failed; abandoning its private store'; root_file="$GITHUB_OUTPUT.mbx-fallback"; if [ -e "$root_file" ] || [ -L "$root_file" ]; then echo "MBX fallback marker already exists" >&2; exit 1; fi; mktemp -d "$RUNNER_TEMP/velnor-mbx-fallback.XXXXXXXXXX" > "$root_file"; IFS= read -r root < "$root_file"; root_id="${root##*/}"; group="velnor-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${root_id}"; printf 'MBX_CACHE_DIR=%s\nMBX_TARGET_ROOT=%s/targets\nMBX_SHIMS_DIR=%s/shims\nMBX_CACHE_EXPORT_GROUP=%s\n' "$root" "$root" "$root" "$group" >> "$GITHUB_ENV"; echo 'fresh cold MBX store selected for subsequent steps'; fi"#;
/// YAML step id for MBX setup and the key/export steps that expose outputs.
pub(crate) fn step_yaml_id(step: &Step) -> Option<&'static str> {
    if is_mbx_action(step) {
        Some("mbx")
    } else {
        match step.name.as_str() {
            MBX_BUNDLE_KEY_NAME => Some("mbx-bundle-key"),
            MBX_BUNDLE_RESTORE_NAME => Some("mbx-bundle"),
            MBX_BUNDLE_EXPORT_NAME => Some("mbx-export"),
            _ => None,
        }
    }
}

/// Emit `id:` for steps whose later steps read outputs.
pub(crate) fn push_step_id(entries: &mut Vec<(String, crate::yaml::Yaml)>, step: &Step) {
    let Some(id) = step_yaml_id(step) else {
        return;
    };
    entries.push(("id".to_owned(), crate::yaml::Yaml::str(id.to_owned())));
}

/// Use the supported local backend and append one explicit bundle lifecycle.
///
/// The runtime key includes the installed MBX version, directory format,
/// runner OS/architecture, rustc identity, and commit. Jobs with the same
/// static runner/version/toolchain selector elect one deterministic writer.
///
/// # Errors
///
/// Returns [`RenderError`] when a job overrides the private MBX store/group
/// environment or when a generated restore/export/save step is invalid.
pub(crate) fn append_single_bundle_saves(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let (identities, winners) = plan_writers(jobs)?;
    for (id, job) in jobs.iter_mut() {
        if !job.steps.iter().any(is_mbx_action) {
            continue;
        }
        let identity = identities.get(id).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("mbx_cache_identity_missing:{id}"))
        })?;
        reject_private_env_overrides(job, id)?;
        pin_local_backend(job, id)?;
        insert_root_step(job)?;
        insert_bundle_restore(job, identity)?;
        if winners.contains(id) {
            insert_writer_steps(job)?;
        } else if has_writer_steps(job) {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_non_elected_writer:{id}"
            )));
        }
    }
    Ok(())
}

/// These names must remain job-owned so later steps cannot redirect MBX or export.
fn reject_private_env_overrides(job: &Job, id: &str) -> Result<(), RenderError> {
    for step in &job.steps {
        let env = match &step.kind {
            StepKind::Action { env, .. } | StepKind::Shell { env, .. } => env,
            StepKind::Internal { .. } => continue,
        };
        if [
            "MBX_CACHE_DIR",
            "MBX_TARGET_ROOT",
            "MBX_SHIMS_DIR",
            "MBX_CACHE_EXPORT_GROUP",
        ]
        .iter()
        .any(|name| env.contains_key(*name))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_private_env_override:{id}:{}",
                step.name
            )));
        }
    }
    Ok(())
}

/// Prevent the stock GitHub backend from restoring/importing/saving internally.
fn pin_local_backend(job: &mut Job, id: &str) -> Result<(), RenderError> {
    for step in &mut job.steps {
        let StepKind::Action { uses, with, env } = &mut step.kind else {
            continue;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            continue;
        }
        if ["isolate-objects-cache", "cache-key-suffix"]
            .iter()
            .any(|input| with.contains_key(*input))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "unsupported_mbx_input:{id}"
            )));
        }
        with.insert("backend".to_owned(), "local".to_owned());
        for input in [
            MBX_SCOPE_INPUT,
            MBX_WRITER_INPUT,
            "github-cache-mode",
            "cache-generation",
            "cache-key",
            "restore-keys",
            "save-on-workflow-dispatch",
            "save-on-pull-request",
            "save-on-protected-branch",
        ] {
            with.remove(input);
        }
        env.remove("ACTIONS_CACHE_MODE");
    }
    Ok(())
}

/// Add a fresh MBX cache root before the earliest MBX action or command.
fn insert_root_step(job: &mut Job) -> Result<(), RenderError> {
    if job.steps.iter().any(|step| step.name == MBX_ROOT_NAME) {
        return Ok(());
    }
    let Some(index) = job.steps.iter().position(|step| {
        is_mbx_action(step)
            || matches!(&step.kind, StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mbx"))
    }) else {
        return Ok(());
    };
    let root = crate::steps::shell_step(
        MBX_ROOT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), ROOT_SCRIPT.to_owned()],
        BTreeMap::new(),
    )?;
    job.steps.insert(index, root);
    Ok(())
}

/// Insert runtime key, restore, and import immediately after local setup.
fn insert_bundle_restore(job: &mut Job, identity: &CacheIdentity) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| step.name == MBX_BUNDLE_RESTORE_NAME)
    {
        return Ok(());
    }
    let Some(index) = job.steps.iter().enumerate().find_map(|(index, step)| {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            return None;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            return None;
        }
        with.get("version")?;
        Some(index)
    }) else {
        return Ok(());
    };
    let qualification_role = identity.qualification_role();
    let added = [
        key_step(
            &identity.generation,
            &identity.version,
            &identity.scope,
            &identity.rust_env,
            qualification_role.is_some(),
        )?,
        restore_step()?,
        import_step(qualification_role)?,
    ];
    let insert_at = index + 1;
    for step in added.into_iter().rev() {
        job.steps.insert(insert_at, step);
    }
    Ok(())
}

fn insert_writer_steps(job: &mut Job) -> Result<(), RenderError> {
    if has_writer_steps(job) {
        return Ok(());
    }
    job.steps.push(export_step()?);
    job.steps.push(save_step()?);
    Ok(())
}

/// Rebind elected save steps to outputs exported by the shared composite call.
///
/// The composite owns the key and restore steps, so their inner step ids are
/// unavailable to the containing workflow job. This is called only for paired
/// lanes; an absent writer is valid on the reader lane.
pub(crate) fn bind_shared_lane_outputs(steps: &mut [Step]) -> Result<(), RenderError> {
    for step in steps {
        if step.name == MBX_BUNDLE_EXPORT_NAME {
            let condition = step.condition.as_mut().ok_or_else(|| {
                RenderError::InvalidWorkflow("mbx_shared_export_condition_missing".to_owned())
            })?;
            rebind_cache_hit(condition)?;
        } else if step.name == MBX_BUNDLE_SAVE_NAME {
            let condition = step.condition.as_mut().ok_or_else(|| {
                RenderError::InvalidWorkflow("mbx_shared_save_condition_missing".to_owned())
            })?;
            rebind_cache_hit(condition)?;
            let StepKind::Action { with, .. } = &mut step.kind else {
                return Err(RenderError::InvalidWorkflow(
                    "mbx_shared_save_not_action".to_owned(),
                ));
            };
            let Some(key) = with.get_mut("key") else {
                return Err(RenderError::InvalidWorkflow(
                    "mbx_shared_save_key_missing".to_owned(),
                ));
            };
            SHARED_CACHE_KEY.clone_into(key);
        }
    }
    Ok(())
}

fn rebind_cache_hit(condition: &mut String) -> Result<(), RenderError> {
    let internal = "steps.mbx-bundle.outputs.cache-hit";
    if !condition.contains(internal) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_shared_cache_hit_reference_missing".to_owned(),
        ));
    }
    *condition = condition.replace(internal, SHARED_CACHE_HIT);
    Ok(())
}

fn has_writer_steps(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME || step.name == MBX_BUNDLE_SAVE_NAME)
}

fn key_step(
    generation: &str,
    version: &str,
    scope: &str,
    rust_env: &BTreeMap<String, String>,
    qualification_nonce: bool,
) -> Result<Step, RenderError> {
    let mut env = BTreeMap::from([
        (
            "MBX_VERSION".to_owned(),
            "${{ steps.mbx.outputs.mbx-version }}".to_owned(),
        ),
        ("MBX_EXPECTED_VERSION".to_owned(), version.to_owned()),
        ("MBX_GENERATION".to_owned(), generation.to_owned()),
        ("MBX_CACHE_SCOPE".to_owned(), scope.to_owned()),
        (
            "MBX_MATRIX_CONTEXT".to_owned(),
            "${{ toJSON(matrix) }}".to_owned(),
        ),
        (
            "MBX_BASE_SHA".to_owned(),
            "${{ github.event.pull_request.base.sha }}".to_owned(),
        ),
    ]);
    env.extend(rust_env.clone());
    let output_script = if qualification_nonce {
        QUALIFICATION_KEY_SCRIPT
    } else {
        KEY_OUTPUT_SCRIPT
    };
    let script = format!("{KEY_SCRIPT}{output_script}");
    crate::steps::shell_step(
        MBX_BUNDLE_KEY_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), script],
        env,
    )
}

fn restore_step() -> Result<Step, RenderError> {
    crate::steps::action_step(
        MBX_BUNDLE_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
            ),
            (
                "restore-keys".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.prefix }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )
}

fn import_step(qualification_role: Option<bool>) -> Result<Step, RenderError> {
    let mut env = BTreeMap::from([(
        "MATCHED".to_owned(),
        "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
    )]);
    let guard = import_guard(qualification_role, &mut env);
    crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            format!("{guard}{IMPORT_SCRIPT}"),
        ],
        env,
    )
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
                "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(SAVE_IF.to_owned());
    Ok(step)
}
