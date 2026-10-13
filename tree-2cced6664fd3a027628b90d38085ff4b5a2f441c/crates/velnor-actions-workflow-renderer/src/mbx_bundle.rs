//! Hosted action-owned MBX isolation and Scale Set single-bundle saves.
//!
//! Hosted Linux jobs disable shared `OUT_DIR` materialization to keep the pinned
//! action's recursive post cleanup writable. Scale Set runners retain the
//! external bundle lifecycle because their store is persistent.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{
    MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, MBX_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES,
    is_mbx_action,
};

/// Display name of the bundle-export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the one-file cache save.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the prefix step ahead of the bundle restore.
pub(crate) const MBX_BUNDLE_KEY_NAME: &str = "Prepare MBX bundle key";
/// Display name of the bundle restore. Same path the save archived.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import into the mbx store.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Bundle path outside the mbx store. `actions/cache` archives only this path.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";
/// Push-only export gate. Exact bundle hits skip; pull requests never reach it.
const PREP_IF: &str = "success() && github.event_name == 'push' && steps.mbx.outputs.cache-hit != 'true' && steps.mbx-bundle.outputs.cache-hit != 'true'";
/// Save gate. Empty exports set `ready=false` and must not call `actions/cache`.
const SAVE_IF: &str = "success() && github.event_name == 'push' && steps.mbx.outputs.cache-hit != 'true' && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'";
/// Only the Linux consumer lane enables action-owned store isolation.
const ISOLATE_HOSTED_CACHE: &str =
    "${{ runner.environment == 'github-hosted' && runner.os == 'Linux' }}";
/// Hosted primary keys are job-private; the Scale Set branch resolves empty.
/// The action appends this suffix after its generated SHA key, while retaining
/// its generated OS/architecture/generation/toolchain restore prefix. Therefore
/// the same job and SHA exact-warm on later runs, and compatible jobs may still
/// warm-start without sharing a primary writer key.
const HOSTED_CACHE_KEY_SUFFIX: &str =
    "${{ runner.environment == 'github-hosted' && github.job || '' }}";
/// Hosted action writes only for protected pushes to the repository default branch.
const HOSTED_CACHE_MODE: &str = "${{ runner.environment == 'github-hosted' && github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}";
/// Shared lane composites keep the external bundle route on Scale Set runners.
const SCALE_SET_ONLY_IF: &str = "runner.environment != 'github-hosted'";
/// Scale Set route: export the receipt closure, then release the local store.
const EXPORT_SCRIPT: &str = r#"set -eu; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; mbx gc; mbx cache dir > "$RUNNER_TEMP/mbx-store-path"; IFS= read -r store < "$RUNNER_TEMP/mbx-store-path"; test -n "$store"; bundle="$RUNNER_TEMP/mbx-single-bundle"; case "$bundle" in "$store"|"$store"/*) exit 1 ;; esac; case "$store" in /|.) exit 1 ;; *mbx*) ;; *) exit 1 ;; esac; rm -rf "$bundle"; if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$RUNNER_TEMP/mbx-export.out" 2>&1; then test -d "$bundle"; rm -rf "$store"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; echo "ready=true" >> "$GITHUB_OUTPUT"; else rm -rf "$bundle"; if grep -q "no completed mbx builds are recorded for export group" "$RUNNER_TEMP/mbx-export.out"; then echo "ready=false" >> "$GITHUB_OUTPUT"; exit 0; fi; cat "$RUNNER_TEMP/mbx-export.out"; exit 1; fi"#;
/// Drop the commit suffix so an older bundle for this toolchain still restores.
const KEY_SCRIPT: &str = r#"set -eu; key="$MBX_KEY"; case "$key" in ''|*-) exit 1 ;; esac; prefix="${key%-*}-"; echo "prefix=${prefix}" >> "$GITHUB_OUTPUT""#;
/// Import when the restore matched. A miss or failed import leaves the local store in place.
const IMPORT_SCRIPT: &str = r#"set -eu; bundle="$RUNNER_TEMP/mbx-single-bundle"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; if [ -z "$MATCHED" ]; then echo "no mbx bundle matched; using local store"; exit 0; fi; if [ ! -d "$bundle" ]; then echo "mbx bundle missing; using local store"; exit 0; fi; if ! mbx cache import "$bundle"; then echo "mbx bundle import failed; using local store"; rm -rf "$bundle"; exit 0; fi"#;

/// Post-report measurement and cleanup for the hosted Linux source cache.
const CARGO_SOURCE_PRUNE_NAME: &str = "Measure and prune Cargo sources";
const CARGO_SOURCE_PRUNE_COMMAND: &str =
    "python3 \"$GITHUB_WORKSPACE/.github/scripts/prune_hosted_cargo_sources.py\"";
const CARGO_SOURCE_PRUNE_IF: &str = "success() && github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && runner.environment == 'github-hosted' && runner.os == 'Linux'";
/// Generated location of the helper used by the protected hosted writer.
pub(crate) const CARGO_SOURCE_PRUNE_PATH: &str = ".github/scripts/prune_hosted_cargo_sources.py";

/// Package the cleanup helper into the consumer's generated tree.
pub(crate) fn hosted_source_prune_file(
    version: &str,
) -> Result<crate::tree::RenderedFile, RenderError> {
    let bytes = crate::marker::with_marker(
        version,
        include_str!("../../../scripts/prune_hosted_cargo_sources.py"),
    )?;
    crate::steps::scan_for_private_subcommands(&bytes)?;
    Ok(crate::tree::RenderedFile {
        path: CARGO_SOURCE_PRUNE_PATH.to_owned(),
        bytes,
    })
}

/// YAML step id for the MBX restore and export steps, when they have one.
pub(crate) fn step_yaml_id(name: &str) -> Option<&'static str> {
    match name {
        MBX_RESTORE_NAME => Some("mbx"),
        MBX_BUNDLE_KEY_NAME => Some("mbx-bundle-key"),
        MBX_BUNDLE_RESTORE_NAME => Some("mbx-bundle"),
        MBX_BUNDLE_EXPORT_NAME => Some("mbx-export"),
        _ => None,
    }
}

/// Emit `id:` for MBX steps whose outputs feed later steps.
pub(crate) fn push_step_id(entries: &mut Vec<(String, crate::yaml::Yaml)>, name: &str) {
    let Some(id) = step_yaml_id(name) else {
        return;
    };
    entries.push(("id".to_owned(), crate::yaml::Yaml::str(id.to_owned())));
}

/// Emit both routes into shared lane composites with runtime runner guards.
///
/// # Errors
///
/// Returns [`RenderError`] when a bundle step fails validation.
pub(crate) fn apply_mbx_cache_policy(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    for job in jobs.values_mut() {
        if !job.steps.iter().any(is_mbx_action) {
            continue;
        }
        configure_runner_scoped_cache(job);
        insert_bundle_restore(job)?;
        if !job
            .steps
            .iter()
            .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME)
        {
            job.steps.push(export_step()?);
            job.steps.push(save_step()?);
        }
    }
    Ok(())
}

/// Append a guarded source-cache cleanup after each hosted Linux job's steps.
pub(crate) fn append_hosted_linux_source_prune(
    jobs: &mut BTreeMap<String, Job>,
    hosted_linux_mbx_jobs: &BTreeSet<String>,
) -> Result<(), RenderError> {
    for id in hosted_linux_mbx_jobs {
        let Some(job) = jobs.get_mut(id) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_source_prune_unknown_job:{id}"
            )));
        };
        if job
            .steps
            .iter()
            .any(|step| step.name == CARGO_SOURCE_PRUNE_NAME)
        {
            continue;
        }
        let env = BTreeMap::from([(
            "CARGO_HOME".to_owned(),
            crate::cache_p08::CARGO_HOME_EXPR.to_owned(),
        )]);
        let mut step = crate::steps::shell_step(
            CARGO_SOURCE_PRUNE_NAME,
            vec![
                "bash".to_owned(),
                "-c".to_owned(),
                CARGO_SOURCE_PRUNE_COMMAND.to_owned(),
            ],
            env,
        )?;
        step.condition = Some(CARGO_SOURCE_PRUNE_IF.to_owned());
        job.steps.push(step);
    }
    Ok(())
}

fn configure_runner_scoped_cache(job: &mut Job) {
    for step in &mut job.steps {
        let StepKind::Action { uses, with, env } = &mut step.kind else {
            continue;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            continue;
        }
        with.insert(
            "isolate-objects-cache".to_owned(),
            ISOLATE_HOSTED_CACHE.to_owned(),
        );
        with.insert(
            "cache-key-suffix".to_owned(),
            HOSTED_CACHE_KEY_SUFFIX.to_owned(),
        );
        env.insert(MBX_CACHE_MODE_ENV.to_owned(), HOSTED_CACHE_MODE.to_owned());
    }
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
    let added = [key_step()?, restore_step()?, import_step()?];
    job.steps.splice(at..at, added);
    Ok(())
}

fn key_step() -> Result<Step, RenderError> {
    let env = BTreeMap::from([(
        "MBX_KEY".to_owned(),
        "${{ steps.mbx.outputs.cache-primary-key }}".to_owned(),
    )]);
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_KEY_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), KEY_SCRIPT.to_owned()],
        env,
    )?;
    step.condition = Some(SCALE_SET_ONLY_IF.to_owned());
    Ok(step)
}

fn restore_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx.outputs.cache-primary-key }}".to_owned(),
            ),
            (
                "restore-keys".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.prefix }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(SCALE_SET_ONLY_IF.to_owned());
    Ok(step)
}

fn import_step() -> Result<Step, RenderError> {
    let env = BTreeMap::from([(
        "MATCHED".to_owned(),
        "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
    )]);
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), IMPORT_SCRIPT.to_owned()],
        env,
    )?;
    step.condition = Some(SCALE_SET_ONLY_IF.to_owned());
    Ok(step)
}

fn export_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), EXPORT_SCRIPT.to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(format!("{SCALE_SET_ONLY_IF} && {PREP_IF}"));
    Ok(step)
}

fn save_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_SAVE_NAME,
        TOOLS_SAVE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx.outputs.cache-primary-key }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(format!("{SCALE_SET_ONLY_IF} && {SAVE_IF}"));
    Ok(step)
}

#[cfg(all(test, unix))]
mod tests {
    use std::process::Command;

    use super::CARGO_SOURCE_PRUNE_COMMAND;

    #[test]
    fn hosted_source_prune_renders_the_owned_linux_helper() {
        assert_eq!(
            CARGO_SOURCE_PRUNE_COMMAND,
            "python3 \"$GITHUB_WORKSPACE/.github/scripts/prune_hosted_cargo_sources.py\""
        );
    }

    #[test]
    fn hosted_source_prune_python_fixtures_pass() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("canonical repository root");
        let output = Command::new("python3")
            .arg("scripts/test_prune_hosted_cargo_sources.py")
            .current_dir(repo_root)
            .output()
            .expect("run source cleanup fixtures");
        assert!(
            output.status.success(),
            "source cleanup fixtures failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
