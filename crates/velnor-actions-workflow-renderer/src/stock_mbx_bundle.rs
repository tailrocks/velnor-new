use std::collections::BTreeMap;

use velnor_actions_contract::{Step, workflow::MbxProcessGroupOperation};

use crate::{RenderError, cache_steps::cache_action_step, steps::mbx_process_group_shell_step};

#[path = "stock_mbx_bundle_cleanup.rs"]
mod cleanup;
#[path = "stock_mbx_bundle_process.rs"]
mod process;

/// One external path is shared by the restore and save cache actions.
pub(crate) const MBX_BUNDLE_CACHE_PATH: &str = crate::cache_steps::MBX_TRANSPORT_CACHE_PATH;
const RESTORE_NAME: &str = "Restore MBX single bundle";
const SAVE_NAME: &str = "Save MBX single bundle";
const TRUSTED_PUSH: &str = "success() && github.event_name == 'push' && toJSON(github.event.repository.fork) == 'false' && github.event.repository.default_branch != '' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";

/// This exact predicate is emitted by export and can be exercised without MBX.
pub(crate) const MBX_OBJECT_PREDICATE: &str = concat!(
    "contains_cache_objects() { root=\"$1\"; ",
    "if [ \"$RUNNER_OS\" = macOS ]; then device=$(stat -f '%d' \"$root\") || return 2; ",
    "elif [ \"$RUNNER_OS\" = Linux ]; then device=$(stat -c '%d' -- \"$root\") || return 2; ",
    "else return 2; fi; [ -n \"$device\" ] || return 2; ",
    "unsafe_entry=$(find -P \"$root\" -xdev ! -type d ! -type f -print -quit) || return 2; ",
    "[ -z \"$unsafe_entry\" ] || return 2; ",
    "find -P \"$root\" -xdev -mindepth 1 -type d -exec sh -c 'device=$1; shift; for path do ",
    "if [ \"$RUNNER_OS\" = macOS ]; then actual=$(stat -f \"%d\" \"$path\") || exit 1; ",
    "elif [ \"$RUNNER_OS\" = Linux ]; then actual=$(stat -c \"%d\" -- \"$path\") || exit 1; ",
    "else exit 1; fi; [ \"$actual\" = \"$device\" ] || exit 1; done' ",
    "sh \"$device\" {} + || return 2; ",
    "find -P \"$root\" -xdev -mindepth 1 -type f -exec sh -c 'device=$1; shift; for path do ",
    "if [ \"$RUNNER_OS\" = macOS ]; then entry=$(stat -f \"%d:%l\" \"$path\") || exit 1; ",
    "elif [ \"$RUNNER_OS\" = Linux ]; then entry=$(stat -c \"%d:%h\" -- \"$path\") || exit 1; ",
    "else exit 1; fi; actual=${entry%%:*}; links=${entry#*:}; ",
    "[ \"$actual\" = \"$device\" ] && [ \"$links\" = 1 ] || exit 1; done' ",
    "sh \"$device\" {} + || return 2; missing_objects=false; ",
    "for base in cas action-results; do if [ -e \"$root/$base\" ] || [ -L \"$root/$base\" ]; then ",
    "[ ! -L \"$root/$base\" ] && [ -d \"$root/$base\" ] || return 2; fi; ",
    "if [ -e \"$root/$base/v1\" ] || [ -L \"$root/$base/v1\" ]; then ",
    "[ ! -L \"$root/$base/v1\" ] && [ -d \"$root/$base/v1\" ] || return 2; fi; ",
    "tree=\"$root/$base/v1/blake3\"; if [ -L \"$tree\" ]; then return 2; fi; ",
    "if [ ! -e \"$tree\" ]; then missing_objects=true; continue; fi; ",
    "[ -d \"$tree\" ] || return 2; ",
    "object=$(find -P \"$tree\" -xdev -type f -links 1 -print -quit) || return 2; ",
    "[ -n \"$object\" ] || missing_objects=true; done; ",
    "[ \"$missing_objects\" = false ] && return 0 || return 1; }"
);

const VALIDATION: [&str; 18] = [
    "fail() { printf '%s\\n' \"$1\" >&2; exit 1; }",
    "canonical_dir() { [ -d \"$1\" ] && [ ! -L \"$1\" ] || return 1; realpath -e \"$1\" | grep -Fqx \"$1\"; }",
    "private_dir() { canonical_dir \"$1\" && find -P \"$1\" -prune -perm 700 -print | grep -Fqx \"$1\"; }",
    "identity_matches() { if [ \"$RUNNER_OS\" = macOS ]; then stat -f '%d:%i' \"$1\" | grep -Fqx \"$2\"; elif [ \"$RUNNER_OS\" = Linux ]; then stat -c '%d:%i' -- \"$1\" | grep -Fqx \"$2\"; else return 1; fi; }",
    "validate_private() { private_dir \"$1\" && identity_matches \"$1\" \"$2\"; }",
    "[ -n \"${RUNNER_TEMP:-}\" ] && [ -n \"${GITHUB_WORKSPACE:-}\" ] || fail 'runner temp and workspace are required'",
    "[ -n \"${GITHUB_OUTPUT:-}\" ] && [ -n \"${GITHUB_ENV:-}\" ] || fail 'GitHub output paths are required'",
    "case \"${GITHUB_REPOSITORY_ID:-}\" in ''|*[!0-9]*) fail 'invalid repository ID' ;; esac",
    "case \"${GITHUB_RUN_ID:-}\" in ''|*[!0-9]*) fail 'invalid run ID' ;; esac",
    "case \"${GITHUB_RUN_ATTEMPT:-}\" in ''|*[!0-9]*) fail 'invalid run attempt' ;; esac",
    "case \"${GITHUB_JOB:-}\" in ''|*[!a-zA-Z0-9_-]*) fail 'invalid job ID' ;; esac",
    "[ \"${VELNOR_MBX_WORKSPACE_ROOT:?}\" = \"$GITHUB_WORKSPACE\" ] && canonical_dir \"$GITHUB_WORKSPACE\" && identity_matches \"$GITHUB_WORKSPACE\" \"${VELNOR_MBX_WORKSPACE_IDENTITY:?}\" && canonical_dir \"$RUNNER_TEMP\" && canonical_dir \"$RUNNER_TEMP/velnor\" || fail 'MBX workspace or runner-temp parent is not canonical'",
    "root=\"$RUNNER_TEMP/velnor/mbx-job-$GITHUB_REPOSITORY_ID-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$GITHUB_JOB\"",
    "[ \"${VELNOR_MBX_JOB_ROOT:?}\" = \"$root\" ] || fail 'MBX job root does not match this job'",
    "job_identity=\"$GITHUB_REPOSITORY_ID:$GITHUB_RUN_ID:$GITHUB_RUN_ATTEMPT:$GITHUB_JOB\"",
    "[ \"${VELNOR_MBX_JOB_IDENTITY:?}\" = \"$job_identity\" ] || fail 'MBX job identity does not match this job'",
    "validate_private \"$root\" \"${VELNOR_MBX_JOB_ROOT_IDENTITY:?}\" || fail 'MBX job root identity is invalid'",
    "group=\"velnor-$GITHUB_REPOSITORY_ID-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$GITHUB_JOB\"",
];

const CHILD_VALIDATION: [&str; 21] = [
    "primary=\"$root/store\"; cold=\"$root/store-cold\"; transport_parent=\"$GITHUB_WORKSPACE/.velnor-mbx-cache\"; bundle=\"$transport_parent/mbx-single-bundle\"; lease=\"$GITHUB_WORKSPACE/.velnor-mbx-cache.lock\"; import_cwd=\"$root/import\"; bundle_identity=\"${VELNOR_MBX_BUNDLE_IDENTITY:?}\"",
    "[ \"${VELNOR_MBX_PRIMARY_CACHE_DIR:?}\" = \"$primary\" ] || fail 'primary MBX store path is invalid'",
    "[ \"${VELNOR_MBX_COLD_CACHE_DIR:?}\" = \"$cold\" ] || fail 'cold MBX store path is invalid'",
    "[ \"${VELNOR_MBX_TRANSPORT_PARENT:?}\" = \"$transport_parent\" ] || fail 'MBX transport parent path is invalid'",
    "[ \"${VELNOR_MBX_TRANSPORT_LEASE:?}\" = \"$lease\" ] || fail 'MBX transport lease path is invalid'",
    "[ \"${VELNOR_MBX_BUNDLE_PATH:?}\" = \"$bundle\" ] || fail 'MBX bundle path is invalid'",
    "[ \"${VELNOR_MBX_IMPORT_CWD:?}\" = \"$import_cwd\" ] || fail 'MBX import cwd is invalid'",
    "[ \"${MBX_CACHE_EXPORT_GROUP:?}\" = \"$group\" ] || fail 'MBX export group is invalid'",
    "validate_private \"$primary\" \"${VELNOR_MBX_PRIMARY_IDENTITY:?}\" || fail 'primary MBX store identity is invalid'",
    "validate_private \"$cold\" \"${VELNOR_MBX_COLD_IDENTITY:?}\" || fail 'cold MBX store identity is invalid'",
    "validate_private \"$transport_parent\" \"${VELNOR_MBX_TRANSPORT_PARENT_IDENTITY:?}\" && validate_private \"$lease\" \"${VELNOR_MBX_TRANSPORT_LEASE_IDENTITY:?}\" || fail 'MBX transport parent or lease identity is invalid'",
    "validate_private \"$import_cwd\" \"${VELNOR_MBX_IMPORT_IDENTITY:?}\" || fail 'MBX import cwd identity is invalid'",
    "bundle_exists() { [ -e \"$bundle\" ] || [ -L \"$bundle\" ]; }",
    "validate_bundle() { validate_private \"$bundle\" \"$bundle_identity\"; }",
    "workspace_path_is_owned() { [ \"$VELNOR_MBX_WORKSPACE_ROOT\" = \"$GITHUB_WORKSPACE\" ] && canonical_dir \"$GITHUB_WORKSPACE\" && identity_matches \"$GITHUB_WORKSPACE\" \"$VELNOR_MBX_WORKSPACE_IDENTITY\"; }",
    "bundle_path_is_direct() { workspace_path_is_owned && [ \"$bundle\" = \"$transport_parent/mbx-single-bundle\" ] && [ \"$VELNOR_MBX_BUNDLE_PATH\" = \"$bundle\" ] && validate_private \"$transport_parent\" \"$VELNOR_MBX_TRANSPORT_PARENT_IDENTITY\" && validate_private \"$lease\" \"$VELNOR_MBX_TRANSPORT_LEASE_IDENTITY\"; }",
    "validate_bundle_child() { bundle_path_is_direct && validate_bundle; }",
    "validate_owned_bundle() { [ -d \"$1\" ] && [ ! -L \"$1\" ] && identity_matches \"$1\" \"$2\" && bundle_path_is_direct; }",
    "clear_target_valid() { if [ \"$1\" = \"$bundle\" ]; then validate_owned_bundle \"$1\" \"$2\"; else validate_private \"$1\" \"$2\"; fi; }",
    concat!(
        "same_device_pair() { if [ \"$RUNNER_OS\" = macOS ]; then stat -f '%d' \"$1\" \"$2\" | uniq -d | grep -q .; elif [ \"$RUNNER_OS\" = Linux ]; then stat -c '%d' -- \"$1\" \"$2\" | uniq -d | grep -q .; else return 1; fi; }; ",
        "same_device_tree() { target=\"$1\"; if [ \"$target\" = \"$bundle\" ]; then anchor=\"$transport_parent\"; else anchor=\"$root\"; fi; same_device_pair \"$anchor\" \"$target\" || return 1; ",
        "find -P \"$target\" -xdev -mindepth 1 -type d -exec sh -c 'anchor=$1; shift; for path do if [ \"$RUNNER_OS\" = macOS ]; then stat -f \"%d\" \"$anchor\" \"$path\" | uniq -d | grep -q . || exit 1; elif [ \"$RUNNER_OS\" = Linux ]; then stat -c \"%d\" -- \"$anchor\" \"$path\" | uniq -d | grep -q . || exit 1; else exit 1; fi; done' sh \"$anchor\" {} + || return 1; ",
        "find -P \"$target\" -xdev -mindepth 1 -type f -exec sh -c 'anchor=$1; shift; for path do if [ \"$RUNNER_OS\" = macOS ]; then stat -f \"%d\" \"$anchor\" \"$path\" | uniq -d | grep -q . || exit 1; elif [ \"$RUNNER_OS\" = Linux ]; then stat -c \"%d\" -- \"$anchor\" \"$path\" | uniq -d | grep -q . || exit 1; else exit 1; fi; done' sh \"$anchor\" {} + || return 1; }"
    ),
    concat!(
        "safe_clear_contents() { target=\"$1\"; identity=\"$2\"; [ \"$target\" = \"$primary\" ] || [ \"$target\" = \"$cold\" ] || [ \"$target\" = \"$bundle\" ] || fail 'cleanup target is not an owned child'; validate_private \"$root\" \"$VELNOR_MBX_JOB_ROOT_IDENTITY\" && clear_target_valid \"$target\" \"$identity\" || fail 'cleanup identity or ownership is invalid'; if [ \"$target\" = \"$bundle\" ]; then bundle_path_is_direct || fail 'workspace bundle ownership changed'; anchor=\"$transport_parent\"; else anchor=\"$root\"; fi; ",
        "same_device_pair \"$anchor\" \"$target\" && chmod u+rwx \"$target\" || fail 'cannot prepare owned cleanup root'; ",
        "find -P \"$target\" -xdev -mindepth 1 -type d -exec sh -c 'anchor=$1; shift; for path do if [ \"$RUNNER_OS\" = macOS ]; then stat -f \"%d\" \"$anchor\" \"$path\" | uniq -d | grep -q . || exit 1; elif [ \"$RUNNER_OS\" = Linux ]; then stat -c \"%d\" -- \"$anchor\" \"$path\" | uniq -d | grep -q . || exit 1; else exit 1; fi; chmod u+rwx \"$path\" || exit 1; done' sh \"$anchor\" {} \\; || fail 'cannot prepare owned cleanup'; ",
        "validate_private \"$root\" \"$VELNOR_MBX_JOB_ROOT_IDENTITY\" && clear_target_valid \"$target\" \"$identity\" && same_device_tree \"$target\" || fail 'cleanup path or device boundary changed'; if [ \"$target\" = \"$bundle\" ]; then bundle_path_is_direct || fail 'workspace bundle ownership changed'; fi; find -P \"$target\" -xdev -depth -mindepth 1 -delete || fail 'cannot clear owned cleanup target'; validate_private \"$root\" \"$VELNOR_MBX_JOB_ROOT_IDENTITY\" && clear_target_valid \"$target\" \"$identity\" || fail 'cleanup identity changed'; if [ \"$target\" = \"$bundle\" ]; then bundle_path_is_direct || fail 'workspace bundle ownership changed'; fi; }"
    ),
];

const BUNDLE_IDENTITY_VALIDATION: [&str; 1] = [
    "refresh_bundle_identity() { bundle_exists && [ -d \"$bundle\" ] && [ ! -L \"$bundle\" ] && bundle_path_is_direct || return 1; if [ \"$RUNNER_OS\" = macOS ]; then observed_bundle_identity=$(stat -f '%d:%i' \"$bundle\") || return 1; elif [ \"$RUNNER_OS\" = Linux ]; then observed_bundle_identity=$(stat -c '%d:%i' -- \"$bundle\") || return 1; else return 1; fi; case \"$observed_bundle_identity\" in ''|*[!0-9:]*) return 1 ;; esac; identity_matches \"$bundle\" \"$observed_bundle_identity\" || return 1; bundle_identity=\"$observed_bundle_identity\"; }",
];

pub(crate) struct MbxBundleCacheSteps {
    /// Restore the external archive, then import it before producers run.
    pub(crate) restore: [Step; 2],
    /// Export only useful objects, then save the same external path.
    pub(crate) save: [Step; 3],
}

pub(crate) fn lifecycle_steps(
    restore_uses: &str,
    save_uses: &str,
    cache_key: &str,
    restore_prefix: &str,
    save_condition: &str,
) -> Result<MbxBundleCacheSteps, RenderError> {
    if save_condition != TRUSTED_PUSH {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_save_condition_not_protected_default_push".to_owned(),
        ));
    }
    ensure_same_cache_pin(restore_uses, save_uses)?;
    let mut restore = cache_action_step(
        true,
        restore_uses,
        "mbx",
        cache_key,
        &[restore_prefix.to_owned()],
        &[MBX_BUNDLE_CACHE_PATH.to_owned()],
    )?;
    RESTORE_NAME.clone_into(&mut restore.name);
    restore.continue_on_error = Some(true);
    let import = import_step()?;
    let mut export = export_step()?;
    export.condition = Some(format!(
        "({save_condition}) && steps.mbx_cache_import.outputs.store_ready == 'true' && steps.mbx_cache_import.outputs.export_allowed == 'true'"
    ));
    let mut save = cache_action_step(
        false,
        save_uses,
        "mbx",
        cache_key,
        &[],
        &[MBX_BUNDLE_CACHE_PATH.to_owned()],
    )?;
    SAVE_NAME.clone_into(&mut save.name);
    save.condition = Some(format!(
        "({save_condition}) && steps.mbx_cache_export.outputs.ready == 'true'"
    ));
    save.continue_on_error = Some(true);
    let cleanup = cleanup::cleanup_step()?;
    Ok(MbxBundleCacheSteps {
        restore: [restore, import],
        save: [export, save, cleanup],
    })
}

fn ensure_same_cache_pin(restore_uses: &str, save_uses: &str) -> Result<(), RenderError> {
    let restore_pin = restore_uses
        .rsplit_once('@')
        .map(|(_, pin)| pin)
        .ok_or_else(|| RenderError::BadActionRef("mbx_restore_ref_missing_pin".to_owned()))?;
    let save_pin = save_uses
        .rsplit_once('@')
        .map(|(_, pin)| pin)
        .ok_or_else(|| RenderError::BadActionRef("mbx_save_ref_missing_pin".to_owned()))?;
    if restore_pin == save_pin {
        Ok(())
    } else {
        Err(RenderError::BadActionRef(
            "mbx_cache_action_pin_mismatch".to_owned(),
        ))
    }
}

fn import_step() -> Result<Step, RenderError> {
    mbx_process_group_shell_step(MbxProcessGroupOperation::Import, &import_env())
}

pub(crate) fn process_group_run(operation: MbxProcessGroupOperation) -> Vec<String> {
    let body = match operation {
        MbxProcessGroupOperation::Import => import_script(),
        MbxProcessGroupOperation::Export => export_script(),
    };
    vec![
        "bash".to_owned(),
        "-c".to_owned(),
        crate::toolchain_env::with_credential_unset_script(&body),
    ]
}

fn import_script() -> String {
    let mut lines = vec!["set -eu".to_owned()];
    lines.extend(VALIDATION.map(str::to_owned));
    lines.extend(CHILD_VALIDATION.map(str::to_owned));
    lines.extend(BUNDLE_IDENTITY_VALIDATION.map(str::to_owned));
    process::append_process_group_control(&mut lines);
    lines.extend(
        [
            "mbx_operation=import",
            "stop_import() { prepare_mbx_signal; stop_mbx_group || return 1; mark_mbx_quiesced || return 1; }",
            "on_import_signal() { signal_status=\"$1\"; prepare_mbx_signal; stop_import || exit 1; exit \"$signal_status\"; }",
            "trap 'on_mbx_exit' EXIT; trap 'on_import_signal 129' HUP; trap 'on_import_signal 130' INT; trap 'on_import_signal 143' TERM",
            "remove_bundle() { if bundle_exists; then refresh_bundle_identity || fail 'restored bundle path is unsafe'; safe_clear_contents \"$bundle\" \"$bundle_identity\"; rmdir \"$bundle\" || fail 'cannot remove empty restored bundle root'; fi; [ ! -e \"$bundle\" ] && [ ! -L \"$bundle\" ] || fail 'restored bundle remains after cleanup'; }",
            "select_store() { selected=\"$1\"; export_allowed=\"$2\"; printf 'MBX_CACHE_DIR=%s\\n' \"$selected\" >> \"$GITHUB_ENV\"; printf 'store_ready=true\\nexport_allowed=%s\\ncache_dir=%s\\nexport_group=%s\\n' \"$export_allowed\" \"$selected\" \"$group\" >> \"$GITHUB_OUTPUT\"; }",
            "[ \"${MBX_CACHE_DIR:?}\" = \"$primary\" ] || fail 'preflight did not select the primary MBX store'",
            "if bundle_exists; then refresh_bundle_identity || fail 'restored MBX bundle path is unsafe'; printf 'bundle_identity=%s\\n' \"$bundle_identity\" >> \"$GITHUB_OUTPUT\"; fi",
            "if [ \"${MBX_CACHE_RESTORE_OUTCOME:?}\" != success ]; then remove_bundle; safe_clear_contents \"$primary\" \"$VELNOR_MBX_PRIMARY_IDENTITY\"; select_store \"$cold\" false; exit 0; fi",
            "if [ -z \"${MBX_CACHE_RESTORE_MATCHED_KEY:-}\" ]; then remove_bundle; select_store \"$primary\" true; exit 0; fi",
            "if ! bundle_exists; then safe_clear_contents \"$primary\" \"$VELNOR_MBX_PRIMARY_IDENTITY\"; select_store \"$cold\" false; exit 0; fi",
            "refresh_bundle_identity || fail 'restored MBX bundle identity is invalid'",
            "if ! private_dir \"$bundle\"; then printf '%s\\n' 'restored MBX bundle mode is incompatible; using the reserved cold store' >&2; remove_bundle; safe_clear_contents \"$primary\" \"$VELNOR_MBX_PRIMARY_IDENTITY\"; select_store \"$cold\" false; exit 0; fi",
            "cd \"$import_cwd\" || fail 'cannot enter isolated MBX import cwd'",
            "if run_mbx cache import \"$bundle\"; then if bundle_exists; then remove_bundle; fi; select_store \"$primary\" true; else printf '%s\\n' 'MBX import unavailable; continuing with the reserved cold store' >&2; if bundle_exists; then remove_bundle; fi; safe_clear_contents \"$primary\" \"$VELNOR_MBX_PRIMARY_IDENTITY\"; select_store \"$cold\" false; fi",
        ]
        .map(str::to_owned),
    );
    lines.join("; ")
}

fn export_step() -> Result<Step, RenderError> {
    let mut step = mbx_process_group_shell_step(MbxProcessGroupOperation::Export, &export_env())?;
    step.condition = Some(format!(
        "({TRUSTED_PUSH}) && steps.mbx_cache_import.outputs.store_ready == 'true' && steps.mbx_cache_import.outputs.export_allowed == 'true'"
    ));
    Ok(step)
}

fn export_script() -> String {
    let mut lines = vec!["set -eu; umask 077".to_owned()];
    lines.extend(VALIDATION.map(str::to_owned));
    lines.extend(CHILD_VALIDATION.map(str::to_owned));
    lines.extend(BUNDLE_IDENTITY_VALIDATION.map(str::to_owned));
    lines.extend(
        [
            "store=\"${MBX_CACHE_DIR:?MBX_CACHE_DIR is required}\"; if [ \"$store\" = \"$primary\" ]; then store_identity=\"$VELNOR_MBX_PRIMARY_IDENTITY\"; elif [ \"$store\" = \"$cold\" ]; then store_identity=\"$VELNOR_MBX_COLD_IDENTITY\"; else fail 'selected MBX store is not job-owned'; fi",
            "validate_private \"$store\" \"$store_identity\" || fail 'selected MBX store identity is invalid'",
            "if bundle_exists; then refresh_bundle_identity || fail 'MBX export path is unsafe'; rmdir \"$bundle\" || fail 'stale MBX bundle is not empty'; fi",
            "cd \"$import_cwd\" || fail 'cannot enter isolated MBX import cwd'",
        ]
        .map(str::to_owned),
    );
    process::append_process_group_control(&mut lines);
    lines.push(MBX_OBJECT_PREDICATE.to_owned());
    lines.extend(
        [
            "identity_captured=false; bundle_identity=; capture_bundle_identity() { if [ \"$identity_captured\" = true ]; then validate_private \"$bundle\" \"$bundle_identity\" && bundle_path_is_direct; return $?; fi; [ -d \"$bundle\" ] && [ ! -L \"$bundle\" ] && bundle_path_is_direct || return 0; if [ \"$RUNNER_OS\" = macOS ]; then identity=$(stat -f '%d:%i' \"$bundle\") || return 1; elif [ \"$RUNNER_OS\" = Linux ]; then identity=$(stat -c '%d:%i' -- \"$bundle\") || return 1; else return 1; fi; case \"$identity\" in ''|*[!0-9:]*) return 1 ;; esac; printf 'bundle_identity=%s\\n' \"$identity\" >> \"$GITHUB_OUTPUT\" || return 1; bundle_identity=\"$identity\"; identity_captured=true; }",
            "mbx_operation=export",
            "stop_export() { prepare_mbx_signal; stop_mbx_group || return 1; capture_bundle_identity || :; mark_cleanup_safe || :; }",
            "on_export_signal() { signal_status=\"$1\"; prepare_mbx_signal; stop_export || exit 1; exit \"$signal_status\"; }",
            "trap 'on_mbx_exit' EXIT; trap 'on_export_signal 129' HUP; trap 'on_export_signal 130' INT; trap 'on_export_signal 143' TERM",
            "if run_mbx cache export --group \"$group\" --format directory \"$bundle\"; then export_status=0; else export_status=$?; fi; capture_bundle_identity || :",
            "if [ \"$export_status\" -ne 0 ]; then if bundle_exists; then [ -n \"$bundle_identity\" ] && validate_private \"$bundle\" \"$bundle_identity\" || fail 'failed MBX export left an unsafe bundle path'; fi; mark_cleanup_safe; printf 'ready=false\\nexport_available=false\\n' >> \"$GITHUB_OUTPUT\"; printf '%s\\n' 'MBX cache export unavailable; skipping cache save' >&2; exit 0; fi",
            "[ -n \"$bundle_identity\" ] && [ -d \"$bundle\" ] && [ ! -L \"$bundle\" ] && bundle_path_is_direct && validate_private \"$bundle\" \"$bundle_identity\" || fail 'MBX export produced an unsafe or non-private bundle path'",
            "mark_cleanup_safe",
            "same_device_tree \"$bundle\" || fail 'MBX export crossed its owned device boundary'",
            "if contains_cache_objects \"$bundle\"; then printf 'ready=true\\nexport_available=true\\n' >> \"$GITHUB_OUTPUT\"; else object_status=$?; if [ \"$object_status\" -eq 1 ]; then printf 'ready=false\\nexport_available=false\\n' >> \"$GITHUB_OUTPUT\"; else fail 'MBX bundle object scan failed closed'; fi; fi",
        ]
        .map(str::to_owned),
    );
    lines.join("; ")
}

fn preflight_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "VELNOR_MBX_JOB_ROOT".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.job_root }}".to_owned(),
        ),
        (
            "VELNOR_MBX_JOB_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.job_identity }}".to_owned(),
        ),
        (
            "VELNOR_MBX_JOB_ROOT_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.job_root_identity }}".to_owned(),
        ),
        (
            "VELNOR_MBX_WORKSPACE_ROOT".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.workspace_root }}".to_owned(),
        ),
        (
            "VELNOR_MBX_WORKSPACE_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.workspace_identity }}".to_owned(),
        ),
        (
            "VELNOR_MBX_TRANSPORT_PARENT".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.transport_parent }}".to_owned(),
        ),
        (
            "VELNOR_MBX_TRANSPORT_PARENT_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.transport_parent_identity }}".to_owned(),
        ),
        (
            "VELNOR_MBX_TRANSPORT_LEASE".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.transport_lease }}".to_owned(),
        ),
        (
            "VELNOR_MBX_TRANSPORT_LEASE_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.transport_lease_identity }}".to_owned(),
        ),
        (
            "VELNOR_MBX_PRIMARY_CACHE_DIR".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.cache_dir }}".to_owned(),
        ),
        (
            "VELNOR_MBX_PRIMARY_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.cache_dir_identity }}".to_owned(),
        ),
        (
            "VELNOR_MBX_COLD_CACHE_DIR".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.cold_cache_dir }}".to_owned(),
        ),
        (
            "VELNOR_MBX_COLD_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.cold_cache_dir_identity }}".to_owned(),
        ),
        (
            "VELNOR_MBX_BUNDLE_PATH".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.bundle_path }}".to_owned(),
        ),
        (
            "VELNOR_MBX_BUNDLE_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.bundle_path_identity }}".to_owned(),
        ),
        (
            "VELNOR_MBX_IMPORT_CWD".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.import_cwd }}".to_owned(),
        ),
        (
            "VELNOR_MBX_IMPORT_IDENTITY".to_owned(),
            "${{ steps.mbx_cache_preflight.outputs.import_cwd_identity }}".to_owned(),
        ),
    ])
}

fn import_env() -> BTreeMap<String, String> {
    let mut env = preflight_env();
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
    env
}

fn export_env() -> BTreeMap<String, String> {
    let mut env = preflight_env();
    env.insert(
        "MBX_CACHE_DIR".to_owned(),
        "${{ steps.mbx_cache_import.outputs.cache_dir }}".to_owned(),
    );
    env.insert(
        "MBX_CACHE_EXPORT_GROUP".to_owned(),
        "${{ steps.mbx_cache_import.outputs.export_group }}".to_owned(),
    );
    env
}

#[cfg(test)]
#[path = "stock_mbx_bundle_tests.rs"]
mod tests;
