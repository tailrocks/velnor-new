//! Hosted action-owned MBX isolation and Scale Set single-bundle saves.
//!
//! Hosted jobs use the action-owned objects backend and write only on a
//! protected default-branch push. Typed Scale Set jobs use the action's local
//! backend plus an independently keyed external bundle route. A miss, a
//! missing directory, or a failed import continues the job cold. Import and
//! export print byte and inode lines for `$RUNNER_TEMP`.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, RunsOn, Step, StepId, StepKind, StepRole};

use crate::RenderError;
use crate::cache_steps::{
    MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, TOOLS_RESTORE_USES, TOOLS_SAVE_USES, is_mbx_action,
};

/// Display name of the reclaim-and-export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the one-file cache save.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the typed MBX cache identity step.
pub(crate) const MBX_CACHE_KEY_NAME: &str = "Prepare MBX cache identity";
/// Local backend setup step emitted for Scale Set jobs.
pub(crate) const MBX_LOCAL_SETUP_NAME: &str = "Prepare MBX local cache store";
/// Display name of the bundle restore. Same path the save archived.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import into the mbx store.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Bundle path outside the mbx store. `actions/cache` archives only this path.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";
/// Only protected default-branch misses produce trusted MBX exports.
fn prepare_condition() -> String {
    format!(
        "success() && github.event_name == 'push' && github.ref == format('refs/heads/{{0}}', github.event.repository.default_branch) && github.ref_protected == true && steps.{}.outputs.cache-hit != 'true'",
        StepId::MbxBundle.as_str()
    )
}

/// Save gate. Empty exports set `ready=false` and must not call `actions/cache`.
fn save_condition() -> String {
    format!(
        "success() && github.event_name == 'push' && github.ref == format('refs/heads/{{0}}', github.event.repository.default_branch) && github.ref_protected == true && steps.{}.outputs.cache-hit != 'true' && steps.{}.outputs.ready == 'true'",
        StepId::MbxBundle.as_str(),
        StepId::MbxExport.as_str()
    )
}
/// One-line script: gc, one external bundle, delete the store only after it exists.
const EXPORT_SCRIPT: &str = r#"set -eu; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; mbx gc; mbx cache dir > "$RUNNER_TEMP/mbx-store-path"; IFS= read -r store < "$RUNNER_TEMP/mbx-store-path"; test -n "$store"; bundle="$RUNNER_TEMP/mbx-single-bundle"; case "$bundle" in "$store"|"$store"/*) exit 1 ;; esac; case "$store" in /|.) exit 1 ;; *mbx*) ;; *) exit 1 ;; esac; rm -rf "$bundle"; if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$RUNNER_TEMP/mbx-export.out" 2>&1; then test -d "$bundle"; rm -rf "$store"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; echo "ready=true" >> "$GITHUB_OUTPUT"; else rm -rf "$bundle"; if grep -q "no completed mbx builds are recorded for export group" "$RUNNER_TEMP/mbx-export.out"; then echo "ready=false" >> "$GITHUB_OUTPUT"; exit 0; fi; cat "$RUNNER_TEMP/mbx-export.out"; exit 1; fi"#;
/// Drop the commit suffix so an older bundle for this toolchain still restores.
const KEY_SCRIPT: &str = r#"set -eu; case "$RUNNER_OS:$RUNNER_ARCH" in Linux:X64) os=linux; arch=x64 ;; Linux:ARM64) os=linux; arch=arm64 ;; macOS:X64) os=darwin; arch=x64 ;; macOS:ARM64) os=darwin; arch=arm64 ;; Windows:X64) os=win32; arch=x64 ;; Windows:ARM64) os=win32; arch=arm64 ;; *) printf 'unsupported MBX runner %s/%s\n' "$RUNNER_OS" "$RUNNER_ARCH" >&2; exit 1 ;; esac; test -n "$CACHE_REVISION"; rust_file="$RUNNER_TEMP/mbx-rustc-identity-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}"; digest_file="${rust_file}-sha256"; rustc "+$RUST_TOOLCHAIN" -vV > "$rust_file"; if [ "$os" = darwin ]; then shasum -a 256 "$rust_file" > "$digest_file"; else sha256sum "$rust_file" > "$digest_file"; fi; IFS=' ' read -r identity _ < "$digest_file"; rm -f "$rust_file" "$digest_file"; identity="${identity:0:12}"; test -n "$identity"; toolchain="rust-${identity}"; key="${os}-${arch}-mbx-${CACHE_GENERATION}-dir-${toolchain}-${GITHUB_JOB}-${CACHE_REVISION}"; case "$key" in ''|*-) exit 1 ;; esac; prefix="${key%-*}-"; printf 'key=%s\nprefix=%s\n' "$key" "$prefix" >> "$GITHUB_OUTPUT"; if [ "$CREATE_EXPORT_GROUP" = true ]; then test -r /proc/sys/kernel/random/uuid; IFS= read -r group_id < /proc/sys/kernel/random/uuid; test -n "$group_id"; group="github-actions-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${group_id}"; printf 'MBX_CACHE_EXPORT_GROUP=%s\n' "$group" >> "$GITHUB_ENV"; fi"#;
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
    Ok(format!(
        r#"set -eu; bundle="$RUNNER_TEMP/mbx-single-bundle"; seed="{seed_root}/mbx"; copy="$RUNNER_TEMP/mbx-seed-bundle"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; if [ -n "$MATCHED" ]; then if [ ! -d "$bundle" ]; then echo "mbx bundle missing; continuing cold"; exit 0; fi; if ! mbx cache import "$bundle"; then echo "mbx bundle import failed; continuing cold"; rm -rf "$bundle"; exit 0; fi; exit 0; fi; if [ -z "$PREFIX" ] || [ ! -f "$seed/PREFIX" ] || [ ! -d "$seed/bundle" ]; then echo "no mbx bundle matched"; exit 0; fi; IFS= read -r seed_prefix < "$seed/PREFIX" || true; if [ "$seed_prefix" != "$PREFIX" ]; then echo "no mbx bundle matched"; exit 0; fi; rm -rf "$copy"; if ! cp -R "$seed/bundle" "$copy"; then echo "mbx bundle import failed; continuing cold"; rm -rf "$copy"; exit 0; fi; if ! mbx cache import "$copy"; then echo "mbx bundle import failed; continuing cold"; rm -rf "$copy"; exit 0; fi; rm -rf "$copy""#
    ))
}

/// Gate hosted cache writes and append the Scale Set single-bundle save.
///
/// # Errors
///
/// Returns [`RenderError`] when the export or save step fails validation.
pub(crate) fn append_single_bundle_saves(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    for job in jobs.values_mut() {
        if !job.steps.iter().any(is_mbx_action) {
            continue;
        }
        let scale_set = configure_runner_scoped_cache(job)?;
        if !scale_set {
            continue;
        }
        insert_bundle_restore(job)?;
        if job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::MbxBundleExport))
        {
            continue;
        }
        job.steps.push(export_step()?);
        job.steps.push(save_step()?);
    }
    Ok(())
}

fn configure_runner_scoped_cache(job: &mut Job) -> Result<bool, RenderError> {
    let runner = RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)?;
    let scale_set = matches!(runner, RunsOn::ScaleSet(_));
    let Some(action_index) = job.steps.iter().position(is_mbx_action) else {
        return Ok(scale_set);
    };
    let (action_sha, mbx_version, rust_toolchain) = {
        let StepKind::Action { uses, with, env } = &job.steps[action_index].kind else {
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
        (action_sha, mbx_version, rust_toolchain)
    };
    if !job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::MbxBundleKey))
    {
        let key = key_step(&action_sha, &mbx_version, &rust_toolchain, scale_set)?;
        job.steps.insert(action_index, key);
    }
    let Some(action_index) = job.steps.iter().position(is_mbx_action) else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_missing_action".to_owned(),
        ));
    };
    let generation = velnor_actions_contract::cachekey::mbx_cache_generation(&mbx_version);
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
        with.remove("cache-key");
        with.remove("restore-keys");
        env.remove(MBX_CACHE_MODE_ENV);
        MBX_LOCAL_SETUP_NAME.clone_into(&mut step.name);
        step.id = None;
        step.role = Some(StepRole::MbxLocalSetup);
    } else {
        with.insert("backend".to_owned(), "github".to_owned());
        with.insert(
            "cache-key".to_owned(),
            output_ref(StepId::MbxBundleKey, "key"),
        );
        with.insert(
            "restore-keys".to_owned(),
            output_ref(StepId::MbxBundleKey, "prefix"),
        );
        env.insert(
            MBX_CACHE_MODE_ENV.to_owned(),
            "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}".to_owned(),
        );
    }
    if !scale_set {
        step.role = Some(StepRole::MbxCache);
    }
    Ok(scale_set)
}

fn insert_bundle_restore(job: &mut Job) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::MbxBundleRestore))
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

fn key_step(
    action_sha: &str,
    mbx_version: &str,
    rust_toolchain: &str,
    create_export_group: bool,
) -> Result<Step, RenderError> {
    let cache_generation = format!(
        "{}-share-out-dir-disabled-v1-action-{action_sha}-dir",
        velnor_actions_contract::cachekey::mbx_cache_generation(mbx_version)
    );
    let env = BTreeMap::from([
        (
            "CREATE_EXPORT_GROUP".to_owned(),
            create_export_group.to_string(),
        ),
        ("CACHE_GENERATION".to_owned(), cache_generation),
        (
            "CACHE_REVISION".to_owned(),
            "${{ github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.sha }}".to_owned(),
        ),
        ("RUST_TOOLCHAIN".to_owned(), rust_toolchain.to_owned()),
    ]);
    let mut step = crate::steps::shell_step(
        MBX_CACHE_KEY_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), KEY_SCRIPT.to_owned()],
        env,
    )?;
    step.id = Some(StepId::MbxBundleKey);
    step.role = Some(StepRole::MbxBundleKey);
    Ok(step)
}

fn restore_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        BTreeMap::from([
            ("key".to_owned(), output_ref(StepId::MbxBundleKey, "key")),
            (
                "restore-keys".to_owned(),
                output_ref(StepId::MbxBundleKey, "prefix"),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.id = Some(StepId::MbxBundle);
    step.role = Some(StepRole::MbxBundleRestore);
    Ok(step)
}

fn import_step() -> Result<Step, RenderError> {
    let env = BTreeMap::from([
        (
            "MATCHED".to_owned(),
            output_ref(StepId::MbxBundle, "cache-matched-key"),
        ),
        (
            "PREFIX".to_owned(),
            output_ref(StepId::MbxBundleKey, "prefix"),
        ),
    ]);
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            import_script(crate::tool_seed::SEED_ROOT)?,
        ],
        env,
    )?;
    step.role = Some(StepRole::MbxBundleImport);
    Ok(step)
}

fn export_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), EXPORT_SCRIPT.to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(prepare_condition());
    step.id = Some(StepId::MbxExport);
    step.role = Some(StepRole::MbxBundleExport);
    Ok(step)
}

fn save_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_SAVE_NAME,
        TOOLS_SAVE_USES,
        BTreeMap::from([
            ("key".to_owned(), output_ref(StepId::MbxBundleKey, "key")),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(save_condition());
    step.role = Some(StepRole::MbxBundleSave);
    Ok(step)
}

/// One typed output reference used by downstream MBX cache steps.
fn output_ref(id: StepId, output: &str) -> String {
    format!("${{{{ steps.{}.outputs.{output} }}}}", id.as_str())
}

#[cfg(test)]
#[path = "mbx_bundle_seed_tests.rs"]
mod seed_tests;
