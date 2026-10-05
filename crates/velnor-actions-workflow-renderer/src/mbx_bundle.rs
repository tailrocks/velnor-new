//! Hosted action-owned MBX isolation and Scale Set single-bundle saves.
//!
//! Hosted jobs use the action-owned objects backend and write only on a
//! protected default-branch push. Typed Scale Set jobs use the action's local
//! backend plus an independently keyed external bundle route. A miss, a
//! missing directory, or a failed import continues the job cold. Import and
//! export print byte and inode lines for `$RUNNER_TEMP`.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, PullRequestCachePolicy, RunsOn, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{
    MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, MBX_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES,
    is_mbx_action,
};
#[path = "mbx_bundle_pr_cache.rs"]
mod pr_cache;

/// Display name of the reclaim-and-export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the one-file cache save.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the typed MBX cache identity step.
pub(crate) const MBX_CACHE_KEY_NAME: &str = "Prepare MBX cache identity";
/// Local backend setup step emitted for Scale Set jobs.
pub(crate) const MBX_LOCAL_SETUP_NAME: &str = "Prepare MBX local cache store";
/// Fresh private store selection for a Scale Set job.
pub(crate) const MBX_PRIVATE_STORE_NAME: &str = "Prepare private MBX store";
/// Display name of the bundle restore. Same path the save archived.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import into the mbx store.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Bundle path outside the mbx store. `actions/cache` archives only this path.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";
/// One-line script: gc, one external bundle, delete the store only after it exists.
const EXPORT_SCRIPT: &str = r#"set -eu; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; mbx gc; mbx cache dir > "$RUNNER_TEMP/mbx-store-path"; IFS= read -r store < "$RUNNER_TEMP/mbx-store-path"; test -n "$store"; bundle="$RUNNER_TEMP/mbx-single-bundle"; case "$bundle" in "$store"|"$store"/*) exit 1 ;; esac; case "$store" in /|.) exit 1 ;; *mbx*) ;; *) exit 1 ;; esac; rm -rf "$bundle"; if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$RUNNER_TEMP/mbx-export.out" 2>&1; then test -d "$bundle"; rm -rf "$store"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; echo "ready=true" >> "$GITHUB_OUTPUT"; else rm -rf "$bundle"; if grep -q "no completed mbx builds are recorded for export group" "$RUNNER_TEMP/mbx-export.out"; then echo "ready=false" >> "$GITHUB_OUTPUT"; exit 0; fi; cat "$RUNNER_TEMP/mbx-export.out"; exit 1; fi"#;
/// Allocate a fresh per-job MBX store under the runner's temporary directory.
const PRIVATE_STORE_SCRIPT: &str = r#"set -eu; test -n "$RUNNER_TEMP"; root_file="$GITHUB_OUTPUT.mbx-root"; if [ -e "$root_file" ] || [ -L "$root_file" ]; then echo "MBX root marker already exists" >&2; exit 1; fi; mktemp -d "$RUNNER_TEMP/velnor-mbx-store.XXXXXXXXXX" > "$root_file"; IFS= read -r root < "$root_file"; case "$root" in "$RUNNER_TEMP"/velnor-mbx-store.*) ;; *) echo "MBX cache root is outside the owned runner-temp namespace" >&2; exit 1 ;; esac; test -d "$root"; test ! -L "$root"; printf 'MBX_CACHE_DIR=%s\nMBX_TARGET_ROOT=%s/targets\nMBX_SHIMS_DIR=%s/shims\n' "$root" "$root" "$root" >> "$GITHUB_ENV"; printf 'selected_cache_root=%s\n' "$root" >> "$GITHUB_OUTPUT""#;
/// Replace a potentially tainted import destination after a failed import.
const FALLBACK_STORE_SCRIPT: &str = r#"fallback_store() { case "$MBX_CACHE_DIR" in "$RUNNER_TEMP"/velnor-mbx-store.*) ;; *) echo "MBX cache root is outside the owned runner-temp namespace" >&2; exit 1 ;; esac; if [ -L "$MBX_CACHE_DIR" ] || [ ! -d "$MBX_CACHE_DIR" ]; then echo "MBX cache root is not an owned directory" >&2; exit 1; fi; rm -rf "$MBX_CACHE_DIR" "$bundle" "$copy"; root_file="$GITHUB_OUTPUT.mbx-fallback"; if [ -e "$root_file" ] || [ -L "$root_file" ]; then echo "MBX fallback marker already exists" >&2; exit 1; fi; mktemp -d "$RUNNER_TEMP/velnor-mbx-fallback.XXXXXXXXXX" > "$root_file"; IFS= read -r root < "$root_file"; case "$root" in "$RUNNER_TEMP"/velnor-mbx-fallback.*) ;; *) echo "MBX fallback root is outside the owned runner-temp namespace" >&2; exit 1 ;; esac; test -d "$root"; test ! -L "$root"; root_id="${root##*/}"; group="velnor-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${root_id}"; printf 'MBX_CACHE_DIR=%s\nMBX_TARGET_ROOT=%s/targets\nMBX_SHIMS_DIR=%s/shims\nMBX_CACHE_EXPORT_GROUP=%s\n' "$root" "$root" "$root" "$group" >> "$GITHUB_ENV"; printf 'selected_cache_root=%s\n' "$root" >> "$GITHUB_OUTPUT"; echo "fresh cold MBX store selected for subsequent steps"; }"#;
/// Import the actions-cache hit, or a private copy of the authorized seed.
///
/// `mbx cache import` removes its input directory. The seed copy is the
/// only directory that command may remove. A miss stays cold.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] when `seed_root` is not a safe
/// absolute path.
pub(crate) fn import_script(seed_root: &str) -> Result<String, RenderError> {
    crate::tool_seed::require_seed_root(seed_root)?;
    let script = [
        "set -eu; bundle=\"$RUNNER_TEMP/mbx-single-bundle\"; seed=\"",
        seed_root,
        "/mbx\"; copy=\"$RUNNER_TEMP/mbx-seed-bundle\"; df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"; if [ -n \"$MATCHED\" ]; then if [ ! -d \"$bundle\" ]; then echo \"mbx bundle missing; continuing cold\"; exit 0; fi; if mbx cache import \"$bundle\"; then printf 'selected_cache_root=%s\\n' \"$MBX_CACHE_DIR\" >> \"$GITHUB_OUTPUT\"; exit 0; fi; echo \"mbx bundle import failed; selecting a fresh cold store\"; ",
        FALLBACK_STORE_SCRIPT,
        "; fallback_store; exit 0; fi; if [ -z \"$PREFIX\" ] || [ ! -f \"$seed/PREFIX\" ] || [ ! -d \"$seed/bundle\" ]; then echo \"no mbx bundle matched\"; printf 'selected_cache_root=%s\\n' \"$MBX_CACHE_DIR\" >> \"$GITHUB_OUTPUT\"; exit 0; fi; IFS= read -r seed_prefix < \"$seed/PREFIX\" || true; if [ \"$seed_prefix\" != \"$PREFIX\" ]; then echo \"no mbx bundle matched\"; printf 'selected_cache_root=%s\\n' \"$MBX_CACHE_DIR\" >> \"$GITHUB_OUTPUT\"; exit 0; fi; rm -rf \"$copy\"; if ! cp -R \"$seed/bundle\" \"$copy\"; then echo \"mbx bundle import failed; selecting a fresh cold store\"; ",
        FALLBACK_STORE_SCRIPT,
        "; fallback_store; exit 0; fi; if ! mbx cache import \"$copy\"; then echo \"mbx bundle import failed; selecting a fresh cold store\"; ",
        FALLBACK_STORE_SCRIPT,
        "; fallback_store; exit 0; fi; rm -rf \"$copy\"; printf 'selected_cache_root=%s\\n' \"$MBX_CACHE_DIR\" >> \"$GITHUB_OUTPUT\"",
    ]
    .concat();
    Ok(script)
}

/// YAML step id for the MBX restore and export steps, when they have one.
pub(crate) fn step_yaml_id(name: &str) -> Option<&'static str> {
    match name {
        MBX_RESTORE_NAME => Some("mbx"),
        MBX_CACHE_KEY_NAME => Some("mbx-cache-key"),
        MBX_PRIVATE_STORE_NAME => Some("mbx-private-store"),
        MBX_BUNDLE_RESTORE_NAME => Some("mbx-bundle"),
        MBX_BUNDLE_EXPORT_NAME => Some("mbx-export"),
        _ => None,
    }
}

/// Emit `id:` for the two MBX steps whose later steps read outputs.
pub(crate) fn push_step_id(entries: &mut Vec<(String, crate::yaml::Yaml)>, name: &str) {
    let Some(id) = step_yaml_id(name) else {
        return;
    };
    entries.push(("id".to_owned(), crate::yaml::Yaml::str(id.to_owned())));
}

/// Gate hosted cache writes and append the Scale Set single-bundle save.
///
/// # Errors
///
/// Returns [`RenderError`] when the export or save step fails validation.
pub(crate) fn append_single_bundle_saves(
    jobs: &mut BTreeMap<String, Job>,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    for job in jobs.values_mut() {
        if !job.steps.iter().any(is_mbx_action) {
            continue;
        }
        let (scale_set, job_policy) =
            configure_runner_scoped_cache(job, pull_request_cache_policy)?;
        if !scale_set {
            continue;
        }
        insert_bundle_restore(job)?;
        if job
            .steps
            .iter()
            .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME)
        {
            continue;
        }
        job.steps.push(export_step(job_policy)?);
        job.steps.push(save_step(job_policy)?);
    }
    Ok(())
}

fn configure_runner_scoped_cache(
    job: &mut Job,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<(bool, PullRequestCachePolicy), RenderError> {
    let runner = RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)?;
    let scale_set = matches!(runner, RunsOn::ScaleSet(_));
    let Some(action_index) = job.steps.iter().position(is_mbx_action) else {
        return Ok((scale_set, pull_request_cache_policy));
    };
    let (action_sha, mbx_version, rust_toolchain) = mbx_action_identity(&job.steps[action_index])?;
    let job_policy = pull_request_cache_policy;
    if !job.steps.iter().any(|step| step.name == MBX_CACHE_KEY_NAME) {
        let key = pr_cache::key_step(
            &action_sha,
            &mbx_version,
            &rust_toolchain,
            scale_set,
            job_policy,
        )?;
        job.steps.insert(action_index, key);
    }
    let Some(mut action_index) = job.steps.iter().position(is_mbx_action) else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_missing_action".to_owned(),
        ));
    };
    if scale_set
        && !job
            .steps
            .iter()
            .any(|step| step.name == MBX_PRIVATE_STORE_NAME)
    {
        job.steps.insert(action_index, private_store_step()?);
        action_index += 1;
    }
    configure_action_cache(
        job,
        action_index,
        scale_set,
        job_policy,
        &action_sha,
        &mbx_version,
    )?;
    Ok((scale_set, job_policy))
}

fn mbx_action_identity(step: &Step) -> Result<(String, String, String), RenderError> {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_bad_action".to_owned(),
        ));
    };
    let action_sha = uses
        .strip_prefix(&format!("{MBX_ACTION_NAME}@"))
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_bundle_bad_action".to_owned()))?
        .to_owned();
    let mbx_version = env
        .get("VELNOR_MBX_VERSION")
        .cloned()
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_missing_version".to_owned()))?;
    let rust_toolchain = with
        .get("toolchain")
        .cloned()
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_missing_toolchain".to_owned()))?;
    Ok((action_sha, mbx_version, rust_toolchain))
}

fn configure_action_cache(
    job: &mut Job,
    action_index: usize,
    scale_set: bool,
    job_policy: PullRequestCachePolicy,
    action_sha: &str,
    mbx_version: &str,
) -> Result<(), RenderError> {
    let generation = velnor_actions_contract::cachekey::mbx_cache_generation(mbx_version);
    let step = &mut job.steps[action_index];
    let StepKind::Action { with, env, .. } = &mut step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_bad_action".to_owned(),
        ));
    };
    with.insert(
        "cache-generation".to_owned(),
        format!("{generation}-share-out-dir-disabled-v1-action-{action_sha}"),
    );
    if scale_set {
        // `local` returns before the pinned action's object restore and
        // leaves the object format to MBX's separate bundle route.
        with.insert("backend".to_owned(), "local".to_owned());
        with.remove("github-cache-mode");
        with.remove("cache-key");
        with.remove("restore-keys");
        env.remove(MBX_CACHE_MODE_ENV);
        MBX_LOCAL_SETUP_NAME.clone_into(&mut step.name);
    } else {
        with.insert("backend".to_owned(), "github".to_owned());
        with.insert(
            "cache-key".to_owned(),
            "${{ steps.mbx-cache-key.outputs.key }}".to_owned(),
        );
        with.insert(
            "restore-keys".to_owned(),
            "${{ steps.mbx-cache-key.outputs.prefix }}".to_owned(),
        );
        env.insert(
            MBX_CACHE_MODE_ENV.to_owned(),
            pr_cache::cache_mode_expression(job_policy),
        );
    }
    Ok(())
}

fn private_store_step() -> Result<Step, RenderError> {
    crate::steps::shell_step(
        MBX_PRIVATE_STORE_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            PRIVATE_STORE_SCRIPT.to_owned(),
        ],
        BTreeMap::new(),
    )
}

fn insert_bundle_restore(job: &mut Job) -> Result<(), RenderError> {
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
    let added = [restore_step()?, import_step()?];
    job.steps.splice(at..at, added);
    Ok(())
}

fn restore_step() -> Result<Step, RenderError> {
    let step = crate::steps::action_step(
        MBX_BUNDLE_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-cache-key.outputs.key }}".to_owned(),
            ),
            (
                "restore-keys".to_owned(),
                "${{ steps.mbx-cache-key.outputs.prefix }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    Ok(step)
}

fn import_step() -> Result<Step, RenderError> {
    let env = BTreeMap::from([
        (
            "MATCHED".to_owned(),
            "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
        ),
        (
            "PREFIX".to_owned(),
            "${{ steps.mbx-cache-key.outputs.prefix }}".to_owned(),
        ),
    ]);
    let step = crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            import_script(crate::tool_seed::SEED_ROOT)?,
        ],
        env,
    )?;
    Ok(step)
}

fn export_step(policy: PullRequestCachePolicy) -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), EXPORT_SCRIPT.to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(pr_cache::prep_condition(policy));
    Ok(step)
}

fn save_step(policy: PullRequestCachePolicy) -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_SAVE_NAME,
        TOOLS_SAVE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-cache-key.outputs.key }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(pr_cache::save_condition(policy));
    Ok(step)
}

#[cfg(test)]
#[path = "mbx_bundle_seed_tests.rs"]
mod seed_tests;
