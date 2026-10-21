use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::{
    CaptureBindingInput, CaptureState, diagnostics_scope, make_binding, persist_state, read_state,
    test_root_path,
};
use crate::launch::completion::diagnostics::DiagnosticsRoot;

#[test]
fn exact_state_temp_is_recovered_without_unlinking_unknown_files() -> Result<(), String> {
    let root_path = test_root_path("state-temp-recovery")?;
    let root = DiagnosticsRoot::open(&root_path, "state-temp-connection")
        .map_err(|error| format!("{error:?}"))?;
    let binding = make_binding(
        &root,
        &CaptureBindingInput {
            intent_id: 13,
            reservation_id: 24,
            operation_id: 35,
            topology_version: 1,
            engine_id: "engine-1",
            docker_root_digest: &"a".repeat(64),
            runner_container_id: &"b".repeat(64),
            dind_container_id: &"c".repeat(64),
            runner_home_volume: "runner-home-24",
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let state = CaptureState::prepared(binding.clone(), 1);
    let bytes = serde_json::to_vec(&state).map_err(|error| error.to_string())?;
    let temporary =
        diagnostics_scope(&root_path, &binding.connection_id).join(".runner-log-capture.tmp");
    fs::write(&temporary, &bytes).map_err(|error| error.to_string())?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;

    persist_state(&root, &binding, &state)?;
    if read_state(&root, &binding)
        .map_err(|error| format!("{error:?}"))?
        .as_ref()
        != Some(&state)
        || temporary.exists()
    {
        return Err("The state writer did not publish the expected state file.".to_owned());
    }
    drop(root);
    fs::remove_dir_all(root_path).map_err(|error| error.to_string())
}

#[test]
fn mismatched_state_temp_is_preserved_and_blocks_state_write() -> Result<(), String> {
    let root_path = test_root_path("state-temp-mismatch")?;
    let root = DiagnosticsRoot::open(&root_path, "state-temp-mismatch-connection")
        .map_err(|error| format!("{error:?}"))?;
    let binding = make_binding(
        &root,
        &CaptureBindingInput {
            intent_id: 14,
            reservation_id: 25,
            operation_id: 36,
            topology_version: 1,
            engine_id: "engine-1",
            docker_root_digest: &"a".repeat(64),
            runner_container_id: &"b".repeat(64),
            dind_container_id: &"c".repeat(64),
            runner_home_volume: "runner-home-25",
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let state = CaptureState::prepared(binding.clone(), 1);
    let temporary =
        diagnostics_scope(&root_path, &binding.connection_id).join(".runner-log-capture.tmp");
    fs::write(&temporary, b"partial state bytes").map_err(|error| error.to_string())?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;

    if persist_state(&root, &binding, &state).is_ok()
        || fs::read(&temporary).map_err(|error| error.to_string())? != b"partial state bytes"
    {
        return Err("The writer accepted or removed an unverified temporary file.".to_owned());
    }
    drop(root);
    fs::remove_dir_all(root_path).map_err(|error| error.to_string())
}
