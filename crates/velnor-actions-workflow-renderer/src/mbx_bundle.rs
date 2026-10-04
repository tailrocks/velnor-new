//! Single-bundle MBX save for hosted and scale-set object caches.
//!
//! `jdx/mr-boxington-action` post saves on every default-branch push, ignoring
//! `save-on-*`. That post exports a directory inside the live store and then
//! asks `actions/cache` to archive it, so the peak is the store, a second full
//! copy, and the cache archive together. `ACTIONS_CACHE_MODE=read` makes
//! `savePolicy` skip that post. A later step reclaims, writes one directory
//! bundle under `runner.temp`, deletes the store only after that bundle
//! exists, and saves that one path. The action restore looks up a different
//! path, so the next job restores this same path and imports it into the store.
//! A miss, a missing directory, or a failed import continues the job cold.
//! Import and export print byte and inode lines for `$RUNNER_TEMP`.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{
    MBX_ACTION_CACHE_MODE, MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, MBX_RESTORE_NAME,
    TOOLS_RESTORE_USES, TOOLS_SAVE_USES, is_mbx_action,
};
use crate::matrix::MATRIX_NEEDS_JOB_ENV;

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
/// Bundle path outside the mbx store. `actions/cache` archives only this path.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";
/// Push-only export gate. Exact bundle hits skip; pull requests never reach it.
const PREP_IF: &str = "success() && github.event_name == 'push' && steps.mbx.outputs.cache-hit != 'true' && steps.mbx-bundle.outputs.cache-hit != 'true'";
/// Save gate. Empty exports set `ready=false` and must not call `actions/cache`.
const SAVE_IF: &str = "success() && github.event_name == 'push' && steps.mbx.outputs.cache-hit != 'true' && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'";

/// One-line script: gc, one external bundle, delete the store only after it exists.
const EXPORT_SCRIPT: &str = r#"set -eu; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; mbx gc; mbx cache dir > "$RUNNER_TEMP/mbx-store-path"; IFS= read -r store < "$RUNNER_TEMP/mbx-store-path"; test -n "$store"; bundle="$RUNNER_TEMP/mbx-single-bundle"; case "$bundle" in "$store"|"$store"/*) exit 1 ;; esac; case "$store" in /|.) exit 1 ;; *mbx*) ;; *) exit 1 ;; esac; rm -rf "$bundle"; if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$RUNNER_TEMP/mbx-export.out" 2>&1; then test -d "$bundle"; rm -rf "$store"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; echo "ready=true" >> "$GITHUB_OUTPUT"; else rm -rf "$bundle"; if grep -q "no completed mbx builds are recorded for export group" "$RUNNER_TEMP/mbx-export.out"; then echo "ready=false" >> "$GITHUB_OUTPUT"; exit 0; fi; cat "$RUNNER_TEMP/mbx-export.out"; exit 1; fi"#;
/// Validate the writer-specific key and keep restore broad across compatible writers.
const KEY_SCRIPT: &str = r#"set -eu; key="$MBX_KEY"; job_id="$MBX_JOB_ID"; matrix_key="$MBX_MATRIX_KEY"; case "$key" in ''|*-) exit 1 ;; esac; case "$job_id" in ''|*[!a-z0-9_-]*) exit 1 ;; esac; suffix="$job_id"; if [ -n "$matrix_key" ]; then case "$matrix_key" in m-????????????????) ;; *) exit 1 ;; esac; case "${matrix_key#m-}" in *[!a-f0-9]*) exit 1 ;; esac; suffix="${suffix}-${matrix_key}"; fi; primary="${key}-${suffix}"; [ "${#primary}" -le 512 ]; prefix="${key%-*}-"; printf 'key=%s\nprefix=%s\n' "$primary" "$prefix" >> "$GITHUB_OUTPUT""#;
/// Import when the restore matched. A miss, a missing directory, or a failed import stays cold.
const IMPORT_SCRIPT: &str = r#"set -eu; bundle="$RUNNER_TEMP/mbx-single-bundle"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; if [ -z "$MATCHED" ]; then echo "no mbx bundle matched"; exit 0; fi; if [ ! -d "$bundle" ]; then echo "mbx bundle missing; continuing cold"; exit 0; fi; if ! mbx cache import "$bundle"; then echo "mbx bundle import failed; continuing cold"; rm -rf "$bundle"; exit 0; fi"#;

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

/// Emit `id:` for the two MBX steps whose later steps read outputs.
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
        pin_restore_only(job);
        let matrix_job = is_matrix_job(job_id, job);
        insert_bundle_restore(job, matrix_job)?;
        if job
            .steps
            .iter()
            .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME)
        {
            continue;
        }
        job.steps.push(export_step()?);
        job.steps.push(save_step()?);
    }
    Ok(())
}

fn pin_restore_only(job: &mut Job) {
    for step in &mut job.steps {
        let StepKind::Action { uses, env, .. } = &mut step.kind else {
            continue;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            continue;
        }
        env.insert(
            MBX_CACHE_MODE_ENV.to_owned(),
            MBX_ACTION_CACHE_MODE.to_owned(),
        );
    }
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
    let added = [key_step(matrix_job)?, restore_step()?, import_step()?];
    job.steps.splice(at..at, added);
    Ok(())
}

fn key_step(matrix_job: bool) -> Result<Step, RenderError> {
    let mut env = BTreeMap::from([
        (
            "MBX_KEY".to_owned(),
            "${{ steps.mbx.outputs.cache-primary-key }}".to_owned(),
        ),
        ("MBX_JOB_ID".to_owned(), "${{ github.job }}".to_owned()),
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
    crate::steps::action_step(
        MBX_BUNDLE_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.key }}".to_owned(),
            ),
            (
                "restore-keys".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.prefix }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )
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
mod tests {
    use std::error::Error;
    use std::process::Command;

    use super::KEY_SCRIPT;

    fn output_for<'a>(script_output: &'a str, name: &str) -> Option<&'a str> {
        script_output
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")))
    }

    fn run_key_script(key: &str, job_id: &str, matrix_key: &str) -> std::io::Result<String> {
        let output = Command::new("bash")
            .args(["-c", KEY_SCRIPT])
            .env("MBX_KEY", key)
            .env("MBX_JOB_ID", job_id)
            .env("MBX_MATRIX_KEY", matrix_key)
            .env("GITHUB_OUTPUT", "/dev/stdout")
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        String::from_utf8(output.stdout).map_err(std::io::Error::other)
    }

    #[test]
    fn writers_get_distinct_keys_and_share_the_unsuffixed_restore_prefix()
    -> Result<(), Box<dyn Error>> {
        let action_key =
            "linux-x64-mbx-generation-rust-1.98.1-0123456789abcdef0123456789abcdef01234567";
        let hosted = run_key_script(action_key, "rust-demo__hosted", "")?;
        let local = run_key_script(action_key, "rust-demo__local", "")?;
        let matrix_a = run_key_script(action_key, "velnor-task", "m-0123456789abcdef")?;
        let matrix_b = run_key_script(action_key, "velnor-task", "m-fedcba9876543210")?;
        let common_prefix = "linux-x64-mbx-generation-rust-1.98.1-";

        assert_eq!(
            output_for(&hosted, "key"),
            Some(
                "linux-x64-mbx-generation-rust-1.98.1-0123456789abcdef0123456789abcdef01234567-rust-demo__hosted"
            )
        );
        assert_eq!(
            output_for(&local, "key"),
            Some(
                "linux-x64-mbx-generation-rust-1.98.1-0123456789abcdef0123456789abcdef01234567-rust-demo__local"
            )
        );
        assert_eq!(
            output_for(&matrix_a, "key"),
            Some(
                "linux-x64-mbx-generation-rust-1.98.1-0123456789abcdef0123456789abcdef01234567-velnor-task-m-0123456789abcdef"
            )
        );
        assert_eq!(
            output_for(&matrix_b, "key"),
            Some(
                "linux-x64-mbx-generation-rust-1.98.1-0123456789abcdef0123456789abcdef01234567-velnor-task-m-fedcba9876543210"
            )
        );
        for output in [&hosted, &local, &matrix_a, &matrix_b] {
            assert_eq!(output_for(output, "prefix"), Some(common_prefix));
        }
        Ok(())
    }
}
