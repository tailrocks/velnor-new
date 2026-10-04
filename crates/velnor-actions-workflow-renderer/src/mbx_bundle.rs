//! Hosted action-owned MBX isolation and scale-set single-bundle saves.
//!
//! Hosted runners use the action's isolated store, per-job cache key, and
//! trusted-main save policy. Scale Set runners retain the external bundle
//! lifecycle because their persistent store must not be replaced by the
//! hosted-only isolation mode.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{
    MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, MBX_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES,
    is_mbx_action,
};

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
/// The shared lane composite enables native isolation only on hosted runners.
const ISOLATE_HOSTED_CACHE: &str = "${{ runner.environment == 'github-hosted' }}";
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
/// Finish collection before the hosted action's post step exports the bundle.
const HOSTED_PRE_EXPORT_GC_IF: &str = "success() && runner.environment == 'github-hosted' && github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";
/// Run after successful hosted Linux main steps, immediately before action post cleanup.
const HOSTED_PRE_POST_CLEANUP_IF: &str =
    "success() && runner.environment == 'github-hosted' && runner.os == 'Linux'";

/// Released action v1.7.1 cleanup leaves MBX output directories read-only.
/// Keep this narrow workaround separate from the upstream action root-cause fix.
const HOSTED_PRE_POST_CLEANUP_SCRIPT: &str = concat!(
    "set -eu; python3 -c 'exec(\"\"\"",
    r#"import errno\n"#,
    r#"import os\n"#,
    r#"import stat\n\n"#,
    r#"DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW\n"#,
    r#"BENIGN_RACES = {errno.ENOENT, errno.ENOTDIR, errno.ELOOP}\n\n"#,
    r#"def open_directory(parent_fd, name):\n"#,
    r#"    try:\n"#,
    r#"        return os.open(name, DIRECTORY_FLAGS, dir_fd=parent_fd)\n"#,
    r#"    except OSError as error:\n"#,
    r#"        if error.errno in BENIGN_RACES:\n"#,
    r#"            return None\n"#,
    r#"        raise\n\n"#,
    r#"def open_components(root_fd, components):\n"#,
    r#"    current_fd = os.dup(root_fd)\n"#,
    r#"    try:\n"#,
    r#"        for name in components:\n"#,
    r#"            child_fd = open_directory(current_fd, name)\n"#,
    r#"            os.close(current_fd)\n"#,
    r#"            if child_fd is None:\n"#,
    r#"                return None\n"#,
    r#"            current_fd = child_fd\n"#,
    r#"        return current_fd\n"#,
    r#"    except BaseException:\n"#,
    r#"        os.close(current_fd)\n"#,
    r#"        raise\n\n"#,
    r#"def add_owner_write(root_fd):\n"#,
    r#"    pending = [()]\n"#,
    r#"    while pending:\n"#,
    r#"        components = pending.pop()\n"#,
    r#"        directory_fd = open_components(root_fd, components)\n"#,
    r#"        if directory_fd is None:\n"#,
    r#"            continue\n"#,
    r#"        try:\n"#,
    r#"            details = os.fstat(directory_fd)\n"#,
    r#"            if not stat.S_ISDIR(details.st_mode):\n"#,
    r#"                continue\n"#,
    r#"            os.fchmod(directory_fd, details.st_mode | stat.S_IWUSR)\n"#,
    r#"            try:\n"#,
    r#"                with os.scandir(directory_fd) as entries:\n"#,
    r#"                    for entry in entries:\n"#,
    r#"                        name = entry.name\n"#,
    r#"                        if name in (\".\", \"..\") or os.sep in name:\n"#,
    r#"                            continue\n"#,
    r#"                        try:\n"#,
    r#"                            is_directory = entry.is_dir(follow_symlinks=False)\n"#,
    r#"                        except OSError as error:\n"#,
    r#"                            if error.errno in BENIGN_RACES:\n"#,
    r#"                                continue\n"#,
    r#"                            raise\n"#,
    r#"                        if is_directory:\n"#,
    r#"                            pending.append(components + (name,))\n"#,
    r#"            except OSError as error:\n"#,
    r#"                if error.errno in BENIGN_RACES:\n"#,
    r#"                    continue\n"#,
    r#"                raise\n"#,
    r#"        finally:\n"#,
    r#"            os.close(directory_fd)\n\n"#,
    r#"def open_runner_temp(path):\n"#,
    r#"    if not os.path.isabs(path):\n"#,
    r#"        raise ValueError(\"RUNNER_TEMP must be absolute\")\n"#,
    r#"    current_fd = os.open(os.sep, DIRECTORY_FLAGS)\n"#,
    r#"    try:\n"#,
    r#"        for component in path.split(os.sep):\n"#,
    r#"            if component in (\"\", \".\"):\n"#,
    r#"                continue\n"#,
    r#"            if component == \"..\":\n"#,
    r#"                raise ValueError(\"RUNNER_TEMP must be normalized\")\n"#,
    r#"            child_fd = open_directory(current_fd, component)\n"#,
    r#"            if child_fd is None:\n"#,
    r#"                os.close(current_fd)\n"#,
    r#"                return None\n"#,
    r#"            os.close(current_fd)\n"#,
    r#"            current_fd = child_fd\n"#,
    r#"        return current_fd\n"#,
    r#"    except BaseException:\n"#,
    r#"        os.close(current_fd)\n"#,
    r#"        raise\n\n"#,
    r#"runner_temp_fd = open_runner_temp(os.environ.get(\"RUNNER_TEMP\", \"\"))\n"#,
    r#"if runner_temp_fd is not None:\n"#,
    r#"    try:\n"#,
    r#"        with os.scandir(runner_temp_fd) as roots:\n"#,
    r#"            for entry in roots:\n"#,
    r#"                root_name = entry.name\n"#,
    r#"                if not root_name.startswith(\"mbx-github-objects-store-\"):\n"#,
    r#"                    continue\n"#,
    r#"                root_fd = open_directory(runner_temp_fd, root_name)\n"#,
    r#"                if root_fd is None:\n"#,
    r#"                    continue\n"#,
    r#"                try:\n"#,
    r#"                    store_fd = open_directory(root_fd, \"store\")\n"#,
    r#"                    if store_fd is None:\n"#,
    r#"                        continue\n"#,
    r#"                    try:\n"#,
    r#"                        out_dirs_fd = open_directory(store_fd, \"out-dirs\")\n"#,
    r#"                        if out_dirs_fd is None:\n"#,
    r#"                            continue\n"#,
    r#"                        try:\n"#,
    r#"                            add_owner_write(out_dirs_fd)\n"#,
    r#"                        finally:\n"#,
    r#"                            os.close(out_dirs_fd)\n"#,
    r#"                    finally:\n"#,
    r#"                        os.close(store_fd)\n"#,
    r#"                finally:\n"#,
    r#"                    os.close(root_fd)\n"#,
    r#"    finally:\n"#,
    r#"        os.close(runner_temp_fd)"#,
    "\"\"\")'",
);

/// One-line script: gc, one external bundle, delete the store only after it exists.
const EXPORT_SCRIPT: &str = r#"set -eu; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; mbx gc; mbx cache dir > "$RUNNER_TEMP/mbx-store-path"; IFS= read -r store < "$RUNNER_TEMP/mbx-store-path"; test -n "$store"; bundle="$RUNNER_TEMP/mbx-single-bundle"; case "$bundle" in "$store"|"$store"/*) exit 1 ;; esac; case "$store" in /|.) exit 1 ;; *mbx*) ;; *) exit 1 ;; esac; rm -rf "$bundle"; if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$RUNNER_TEMP/mbx-export.out" 2>&1; then test -d "$bundle"; rm -rf "$store"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; echo "ready=true" >> "$GITHUB_OUTPUT"; else rm -rf "$bundle"; if grep -q "no completed mbx builds are recorded for export group" "$RUNNER_TEMP/mbx-export.out"; then echo "ready=false" >> "$GITHUB_OUTPUT"; exit 0; fi; cat "$RUNNER_TEMP/mbx-export.out"; exit 1; fi"#;
/// Drop the commit suffix so an older bundle for this toolchain still restores.
const KEY_SCRIPT: &str = r#"set -eu; key="$MBX_KEY"; case "$key" in ''|*-) exit 1 ;; esac; prefix="${key%-*}-"; echo "prefix=${prefix}" >> "$GITHUB_OUTPUT""#;
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
        if job
            .steps
            .iter()
            .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME)
        {
            continue;
        }
        job.steps.push(export_step()?);
        job.steps.push(save_step()?);
        job.steps.push(pre_export_gc_step()?);
        job.steps.push(pre_post_cleanup_step()?);
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

fn pre_export_gc_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        "Collect MBX cache before export",
        vec!["mbx".to_owned(), "gc".to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(HOSTED_PRE_EXPORT_GC_IF.to_owned());
    Ok(step)
}

fn pre_post_cleanup_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        "Normalize MBX directories before action post",
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            HOSTED_PRE_POST_CLEANUP_SCRIPT.to_owned(),
        ],
        BTreeMap::new(),
    )?;
    step.condition = Some(HOSTED_PRE_POST_CLEANUP_IF.to_owned());
    Ok(step)
}
