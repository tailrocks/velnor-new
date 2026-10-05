use super::*;
use crate::cache_steps::{TOOLS_RESTORE_USES, TOOLS_SAVE_USES};
use velnor_actions_contract::{StepKind, workflow::MbxProcessGroupOperation};

#[path = "stock_mbx_bundle_mode_zero_tests.rs"]
mod mode_zero_tests;

const KEY: &str = "velnor-v1-mbx-trusted-linux-x64-rust-1.98.1-mbx-1.21.1-directory-clippy-b3-compat-${{hashFiles('Cargo.lock')}}";
const PREFIX: &str =
    "velnor-v1-mbx-trusted-linux-x64-rust-1.98.1-mbx-1.21.1-directory-clippy-b3-compat-";
fn steps() -> Result<MbxBundleCacheSteps, RenderError> {
    lifecycle_steps(
        TOOLS_RESTORE_USES,
        TOOLS_SAVE_USES,
        KEY,
        PREFIX,
        TRUSTED_PUSH,
    )
}
fn action(step: &Step) -> Option<(&str, &BTreeMap<String, String>)> {
    match &step.kind {
        StepKind::Action { uses, with, .. } => Some((uses, with)),
        _ => None,
    }
}
fn shell(step: &Step) -> Option<(String, &BTreeMap<String, String>)> {
    match &step.kind {
        StepKind::Shell { run, env } => Some((inline_script(run)?, env)),
        StepKind::MbxProcessGroupShell(payload) => {
            let run = process_group_run(payload.operation());
            Some((inline_script(&run)?, payload.env()))
        }
        _ => None,
    }
}
fn inline_script(run: &[String]) -> Option<String> {
    (run.len() == 3 && run[1] == "-c").then(|| run[2].clone())
}
fn process_group_operation(step: &Step) -> Option<MbxProcessGroupOperation> {
    match &step.kind {
        StepKind::MbxProcessGroupShell(payload) => Some(payload.operation()),
        _ => None,
    }
}
#[test]
fn restore_save_use_one_pinned_path_and_restore_failure_is_soft() -> Result<(), RenderError> {
    let lifecycle = steps()?;
    let restore = action(&lifecycle.restore[0])
        .ok_or_else(|| RenderError::InvalidWorkflow("expected_restore_action".to_owned()))?;
    let save = action(&lifecycle.save[1])
        .ok_or_else(|| RenderError::InvalidWorkflow("expected_save_action".to_owned()))?;
    assert_eq!(lifecycle.restore[0].name, RESTORE_NAME);
    assert_eq!(lifecycle.restore[0].continue_on_error, Some(true));
    assert_eq!(lifecycle.restore[1].name, "Import MBX single bundle");
    assert_eq!(lifecycle.save[0].name, "Export MBX single bundle");
    assert_eq!(
        process_group_operation(&lifecycle.restore[1]),
        Some(MbxProcessGroupOperation::Import)
    );
    assert_eq!(
        process_group_operation(&lifecycle.save[0]),
        Some(MbxProcessGroupOperation::Export)
    );
    assert_eq!(lifecycle.save[1].name, SAVE_NAME);
    assert_eq!(lifecycle.save[1].continue_on_error, Some(true));
    assert_eq!(lifecycle.restore[1].continue_on_error, None);
    assert_eq!(lifecycle.save[0].continue_on_error, None);
    assert_eq!(restore.0, TOOLS_RESTORE_USES);
    assert_eq!(save.0, TOOLS_SAVE_USES);
    assert_eq!(restore.1.get("key"), save.1.get("key"));
    assert_eq!(restore.1.get("path"), save.1.get("path"));
    assert_eq!(
        restore.1.get("path").map(String::as_str),
        Some(MBX_BUNDLE_CACHE_PATH)
    );
    assert_eq!(
        restore.1.get("restore-keys").map(String::as_str),
        Some(PREFIX)
    );
    Ok(())
}
#[test]
fn import_cleans_failed_private_inputs_and_selects_the_reserved_cold_store()
-> Result<(), RenderError> {
    let lifecycle = steps()?;
    let (script, env) = shell(&lifecycle.restore[1])
        .ok_or_else(|| RenderError::InvalidWorkflow("expected_import_shell".to_owned()))?;
    assert_eq!(
        env.get("MBX_CACHE_RESTORE_OUTCOME").map(String::as_str),
        Some("${{ steps.mbx_cache_restore.outcome }}")
    );
    assert_eq!(
        env.get("MBX_CACHE_RESTORE_MATCHED_KEY").map(String::as_str),
        Some("${{ steps.mbx_cache_restore.outputs.cache-matched-key }}")
    );
    assert!(script.contains("find -P \"$target\" -xdev -depth -mindepth 1 -delete"));
    assert!(script.contains("safe_clear_contents \"$primary\" \"$VELNOR_MBX_PRIMARY_IDENTITY\""));
    assert!(script.contains("select_store \"$cold\""));
    assert!(script.contains("select_store \"$cold\" false"));
    assert!(script.contains("store_ready=true\\nexport_allowed=%s"));
    assert!(
        script
            .contains("refresh_bundle_identity || fail 'restored MBX bundle identity is invalid'")
    );
    assert!(script.contains("if ! bundle_exists; then safe_clear_contents \"$primary\""));
    assert!(script.contains("if ! private_dir \"$bundle\"; then printf '%s\\n' 'restored MBX bundle mode is incompatible"));
    assert!(script.contains("select_store \"$cold\" false; exit 0; fi"));
    assert!(script.contains("mbx_quiesced=true\\n"));
    assert!(script.contains("trap 'on_mbx_exit' EXIT"));
    assert!(script.contains("capture_mbx_group()"));
    assert!(script.contains("group_has_live_processes \"$mbx_pgid\""));
    assert!(script.contains("$1 == child { direct=1; if ($2 != group) mismatch=1 }"));
    assert!(script.contains("END { if (mismatch) exit 3; if (direct || active) exit 0; exit 1 }"));
    assert!(script.contains("stop_unverified_mbx_child \"$mbx_pid\""));
    assert!(script.contains("set -m"));
    assert!(script.contains("mbx \"$@\" & mbx_pid=$!"));
    assert!(
        script.contains("kill -TERM -- \"-$target_group\"")
            && script.contains("wait \"$target_pid\"")
    );
    assert!(
        script.contains("kill -KILL -- \"-$target_group\"")
            && script.contains("ps -A -o pid= -o pgid= -o stat=")
    );
    assert!(script.contains("cd \"$import_cwd\""));
    assert!(!script.contains("rm -rf") && !script.contains("mbx gc"));
    assert_eq!(
        env.get("VELNOR_MBX_JOB_ROOT_IDENTITY").map(String::as_str),
        Some("${{ steps.mbx_cache_preflight.outputs.job_root_identity }}")
    );
    Ok(())
}
#[test]
fn export_requires_trusted_objects_and_emits_unavailable_for_empty_output()
-> Result<(), RenderError> {
    let lifecycle = steps()?;
    let (script, _) = shell(&lifecycle.save[0])
        .ok_or_else(|| RenderError::InvalidWorkflow("expected_export_shell".to_owned()))?;
    assert_eq!(lifecycle.save[0].condition.as_deref(), Some(format!("({TRUSTED_PUSH}) && steps.mbx_cache_import.outputs.store_ready == 'true' && steps.mbx_cache_import.outputs.export_allowed == 'true'").as_str()));
    assert!(
        script.contains("set -eu; umask 077")
            && script.contains("set -m")
            && script.contains("mbx \"$@\" & mbx_pid=$!")
            && script
                .contains("run_mbx cache export --group \"$group\" --format directory \"$bundle\"")
    );
    assert!(script.contains("trap 'on_export_signal 143' TERM"));
    assert!(script.contains("capture_bundle_identity || :; if [ \"$export_status\" -ne 0 ]"));
    assert!(script.contains("mark_cleanup_safe"));
    assert!(script.contains("if contains_cache_objects \"$bundle\""));
    assert!(script.contains("object_status=$?; if [ \"$object_status\" -eq 1 ]"));
    assert!(script.contains("MBX bundle object scan failed closed"));
    assert!(script.contains(
        "same_device_tree \"$bundle\" || fail 'MBX export crossed its owned device boundary'"
    ));
    assert!(script.contains(MBX_OBJECT_PREDICATE) && !MBX_OBJECT_PREDICATE.contains("| grep"));
    assert!(MBX_OBJECT_PREDICATE.contains("-type d -exec sh -c"));
    assert!(MBX_OBJECT_PREDICATE.contains("-type f -exec sh -c"));
    assert!(MBX_OBJECT_PREDICATE.contains("stat -f \"%d:%l\""));
    assert!(MBX_OBJECT_PREDICATE.contains("stat -c \"%d:%h\""));
    assert!(MBX_OBJECT_PREDICATE.contains("find -P \"$root\" -xdev"));
    assert!(MBX_OBJECT_PREDICATE.contains("find -P \"$tree\" -xdev"));
    assert!(MBX_OBJECT_PREDICATE.contains("stat -f \"%d\" \"$path\""));
    assert!(MBX_OBJECT_PREDICATE.contains("stat -c \"%d\" -- \"$path\""));
    assert!(MBX_OBJECT_PREDICATE.contains("|| return 2"));
    assert!(MBX_OBJECT_PREDICATE.contains("missing_objects=true"));
    assert!(MBX_OBJECT_PREDICATE.contains("return 0 || return 1"));
    assert!(script.contains("for base in cas action-results"));
    assert!(script.contains("/v1/blake3"));
    assert!(script.contains("export_available=false") && script.contains("export_available=true"));
    assert!(script.contains("stale MBX bundle is not empty"));
    assert!(!script.contains("rm -rf") && !script.contains("mbx gc"));
    assert_eq!(lifecycle.save[1].continue_on_error, Some(true));
    assert_eq!(lifecycle.save[2].name, "Cleanup MBX single bundle");
    assert!(
        lifecycle.save[2]
            .condition
            .as_deref()
            .is_some_and(|condition| condition.starts_with("always()"))
    );
    Ok(())
}

#[test]
fn untrusted_save_conditions_and_mismatched_cache_pins_are_rejected() {
    for condition in [
        "success() && github.event_name == 'pull_request'",
        "success() && github.event_name == 'merge_group'",
        "success() && github.event_name == 'push' && github.event.repository.fork == false",
        "success() && github.event_name == 'push'",
    ] {
        assert!(matches!(
            lifecycle_steps(TOOLS_RESTORE_USES, TOOLS_SAVE_USES, KEY, PREFIX, condition),
            Err(RenderError::InvalidWorkflow(problem)) if problem == "mbx_bundle_save_condition_not_protected_default_push"
        ));
    }
    assert!(matches!(
        lifecycle_steps(
            "actions/cache/restore@v6.1.0",
            "actions/cache/save@v6.2.0",
            KEY,
            PREFIX,
            TRUSTED_PUSH
        ),
        Err(RenderError::BadActionRef(problem))
            if problem == "mbx_cache_action_pin_mismatch"
    ));
}

#[test]
fn generic_shell_factory_rejects_same_name_process_group_spoof() {
    let script = "set -m; sleep 1 & wait".to_owned();
    assert!(matches!(
        crate::steps::shell_step(
            "Import MBX single bundle",
            vec!["bash".to_owned(), "-c".to_owned(), script],
            BTreeMap::new(),
        ),
        Err(RenderError::BadCommand(problem)) if problem.starts_with("background_shell:")
    ));
}

#[test]
fn cleanup_requires_quiesced_children_and_never_reuses_replaced_preflight_identity()
-> Result<(), RenderError> {
    let lifecycle = steps()?;
    let (script, env) = shell(&lifecycle.save[2])
        .ok_or_else(|| RenderError::InvalidWorkflow("expected_cleanup_shell".to_owned()))?;
    assert!(script.contains("$VELNOR_MBX_EXPORT_CLEANUP_SAFE\" = true"));
    assert!(script.contains("$VELNOR_MBX_IMPORT_QUIESCED\" = true"));
    assert!(script.contains("$VELNOR_MBX_EXPORT_OUTCOME\" != failure"));
    assert!(script.contains("$VELNOR_MBX_IMPORT_OUTCOME\" != failure"));
    assert!(script.contains("unsafe MBX export failure blocks cleanup"));
    assert!(script.contains("unsafe MBX import failure blocks cleanup"));
    assert!(script.contains("same_device_tree()"));
    assert!(script.contains("validate_owned_bundle"));
    let clear_start = script
        .find("safe_clear_contents() {")
        .ok_or_else(|| RenderError::InvalidWorkflow("cleanup_function_missing".to_owned()))?;
    let clear_script = &script[clear_start..];
    let owner_gate = clear_script
        .find("clear_target_valid \"$target\" \"$identity\" || fail 'cleanup identity or ownership is invalid'")
        .ok_or_else(|| RenderError::InvalidWorkflow("cleanup_owner_gate_missing".to_owned()))?;
    let root_chmod = clear_script
        .find("same_device_pair \"$anchor\" \"$target\" && chmod u+rwx \"$target\"")
        .ok_or_else(|| RenderError::InvalidWorkflow("cleanup_root_chmod_missing".to_owned()))?;
    let first_chmod = clear_script
        .find("find -P \"$target\" -xdev -mindepth 1 -type d -exec sh -c")
        .ok_or_else(|| RenderError::InvalidWorkflow("cleanup_chmod_walk_missing".to_owned()))?;
    let second_device_gate = clear_script
        .find("&& same_device_tree \"$target\" || fail 'cleanup path or device boundary changed'")
        .ok_or_else(|| RenderError::InvalidWorkflow("cleanup_recheck_missing".to_owned()))?;
    let delete_walk = clear_script
        .find("find -P \"$target\" -xdev -depth -mindepth 1 -delete")
        .ok_or_else(|| RenderError::InvalidWorkflow("cleanup_delete_walk_missing".to_owned()))?;
    assert!(owner_gate < root_chmod && root_chmod < first_chmod);
    assert!(
        clear_script[first_chmod..]
            .contains(" \"$anchor\" {} \\; || fail 'cannot prepare owned cleanup'")
    );
    assert!(first_chmod < second_device_gate && second_device_gate < delete_walk);
    assert!(
        script.contains("stat -f \"%d\" \"$anchor\" \"$path\" | uniq -d | grep -q . || exit 1")
    );
    assert!(
        script.contains("stat -c \"%d\" -- \"$anchor\" \"$path\" | uniq -d | grep -q . || exit 1")
    );
    assert!(script.contains("find -P \"$target\" -xdev"));
    assert!(!script.contains("$("));
    assert!(script.contains("workspace bundle cleanup identity is unavailable"));
    assert!(script.contains("[ \"$VELNOR_MBX_RESTORE_OUTCOME\" = cancelled ]"));
    assert!(script.contains("[ \"$VELNOR_MBX_RESTORE_OUTCOME\" = skipped ]"));
    assert!(script.contains("[ \"$VELNOR_MBX_IMPORT_OUTCOME\" = skipped ]"));
    assert!(script.contains("[ \"$VELNOR_MBX_EXPORT_OUTCOME\" = skipped ]"));
    assert!(script.contains("preflight bundle identity changed before restore"));
    assert!(script.contains("safe_clear_contents \"$primary\" \"$VELNOR_MBX_PRIMARY_IDENTITY\""));
    assert!(script.contains("safe_clear_contents \"$cold\" \"$VELNOR_MBX_COLD_IDENTITY\""));
    assert_eq!(
        env.get("VELNOR_MBX_EXPORTED_BUNDLE_IDENTITY")
            .map(String::as_str),
        Some("${{ steps.mbx_cache_export.outputs.bundle_identity }}")
    );
    assert_eq!(
        env.get("VELNOR_MBX_IMPORTED_BUNDLE_IDENTITY")
            .map(String::as_str),
        Some("${{ steps.mbx_cache_import.outputs.bundle_identity }}")
    );
    assert_eq!(
        env.get("VELNOR_MBX_EXPORT_CLEANUP_SAFE")
            .map(String::as_str),
        Some("${{ steps.mbx_cache_export.outputs.cleanup_safe }}")
    );
    assert_eq!(
        env.get("VELNOR_MBX_IMPORT_QUIESCED").map(String::as_str),
        Some("${{ steps.mbx_cache_import.outputs.mbx_quiesced }}")
    );
    Ok(())
}
