use std::path::Path;
use std::process::ExitCode;

use super::super::{
    LinuxDrainSettings, LinuxDrainStatus, disconnect_for_os, drain_for_os,
    linux_drain_requested_message, linux_drain_unknown_message, linux_drain_with, requested_wait,
    resume_for_os,
};
use super::disconnect_state_path;

#[test]
fn disconnect_requires_explicit_drain_and_wait() {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, false, false, None, "macos"),
        ExitCode::from(2)
    );
    assert!(!state.exists());
}

#[test]
fn macos_disconnect_records_only_the_legacy_marker_and_never_claims_disconnect()
-> Result<(), String> {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, true, true, Some(30), "macos"),
        ExitCode::from(1)
    );
    let marker = std::fs::read(state.join("drain")).map_err(|error| error.to_string())?;
    assert_eq!(marker, b"1");
    assert_eq!(requested_wait(Some(30)), "the requested 30-second");
    assert_eq!(
        velnor_runner_host::disconnect_effects(velnor_runner_host::SetOwnership::Adopted, true),
        vec![velnor_runner_host::DisconnectEffect::Drain]
    );
    std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn linux_disconnect_wait_fails_without_remote_or_marker_effects() {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, true, true, None, "linux"),
        ExitCode::from(1)
    );
    assert!(!state.exists());
    assert_eq!(requested_wait(None), "the configured-timeout");
}

#[test]
fn linux_drain_fences_before_wait_and_passes_the_same_absolute_deadline() {
    use std::cell::Cell;

    use velnor_runner_launch::launch::control::{DrainOutcome, DrainRequestOutcome, DrainUnknown};

    let requested = Cell::new(false);
    let status = linux_drain_with(
        &(),
        Path::new("/state/launch.db"),
        true,
        Some(30),
        &LinuxDrainSettings {
            docker_endpoint: "unix:///var/run/docker.sock".to_owned(),
            configured_timeout_secs: 900,
        },
        |(), path, deadline| {
            assert_eq!(path, Path::new("/state/launch.db"));
            assert!(deadline > std::time::Instant::now());
            requested.set(true);
            Ok(DrainRequestOutcome::Requested)
        },
        |(), path, endpoint, deadline| {
            assert!(
                requested.get(),
                "the waiter must run after fence confirmation"
            );
            assert_eq!(path, Path::new("/state/launch.db"));
            assert_eq!(endpoint, "unix:///var/run/docker.sock");
            assert!(deadline > std::time::Instant::now());
            DrainOutcome::Unknown(DrainUnknown::OwnershipInventoryUnavailable)
        },
    );

    assert_eq!(
        status,
        LinuxDrainStatus::WaitUnknown(DrainUnknown::OwnershipInventoryUnavailable)
    );
}

#[test]
fn linux_drain_without_wait_records_only_the_durable_request() {
    use velnor_runner_launch::launch::control::DrainRequestOutcome;

    let status = linux_drain_with(
        &(),
        Path::new("/state/launch.db"),
        false,
        None,
        &LinuxDrainSettings {
            docker_endpoint: "unix:///var/run/docker.sock".to_owned(),
            configured_timeout_secs: 60,
        },
        |(), _, deadline| {
            assert!(deadline > std::time::Instant::now());
            Ok(DrainRequestOutcome::Requested)
        },
        |(), _, _, _| panic!("non-waiting drain must not enter the waiter"),
    );
    assert_eq!(status, LinuxDrainStatus::Requested);
    assert_eq!(
        linux_drain_requested_message(),
        "drain_request_persisted; active_daemon_enforcement_and_quiescence_are_not_proven"
    );
}

#[test]
fn linux_drain_never_waits_after_an_ambiguous_request() {
    use velnor_runner_launch::launch::control::DrainRequestOutcome;

    let status = linux_drain_with(
        &(),
        Path::new("/state/launch.db"),
        true,
        Some(10),
        &LinuxDrainSettings {
            docker_endpoint: "unix:///var/run/docker.sock".to_owned(),
            configured_timeout_secs: 60,
        },
        |(), _, _| Ok(DrainRequestOutcome::UnknownAfterMutation),
        |(), _, _, _| panic!("ambiguous request must not be followed by a wait"),
    );
    assert_eq!(status, LinuxDrainStatus::RequestUnknown);
}

#[test]
fn linux_drain_unknown_reports_fence_state_accurately() {
    use velnor_runner_launch::launch::control::DrainUnknown;

    assert_eq!(
        linux_drain_unknown_message(DrainUnknown::StateDirectoryUnavailable),
        "drain_unknown:state_directory_unavailable; admission fence state is not proven"
    );
    assert_eq!(
        linux_drain_unknown_message(DrainUnknown::AdmissionNotFenced),
        "drain_unknown:admission_not_fenced; admission fence state is not proven"
    );
    assert_eq!(
        linux_drain_unknown_message(DrainUnknown::JournalUnavailable),
        "drain_unknown:journal_unavailable; admission fence state is not proven"
    );
    assert_eq!(
        linux_drain_unknown_message(DrainUnknown::RuntimeUnavailable),
        "drain_unknown:runtime_unavailable; admission fence state is not proven"
    );
    assert_eq!(
        linux_drain_unknown_message(DrainUnknown::OwnershipInventoryUnavailable),
        "drain_unknown:ownership_inventory_unavailable; admission fence state is not proven"
    );
}

#[test]
fn linux_drain_rejects_zero_timeout_before_request() {
    use velnor_runner_launch::launch::control::{ControlOpenError, DrainRequestOutcome};

    let status = linux_drain_with(
        &(),
        Path::new("/state/launch.db"),
        true,
        Some(0),
        &LinuxDrainSettings {
            docker_endpoint: "unix:///var/run/docker.sock".to_owned(),
            configured_timeout_secs: 60,
        },
        |(), _, _| -> Result<DrainRequestOutcome, ControlOpenError> {
            panic!("invalid deadline must not request drain")
        },
        |(), _, _, _| panic!("invalid deadline must not wait"),
    );
    assert_eq!(status, LinuxDrainStatus::InvalidDeadline);
}

#[test]
fn macos_drain_keeps_the_legacy_marker_but_does_not_claim_enforcement() -> Result<(), String> {
    let state = disconnect_state_path();
    assert_eq!(
        drain_for_os(&state, Path::new("/unused"), false, None, "macos"),
        ExitCode::from(1)
    );
    assert_eq!(
        std::fs::read(state.join("drain")).map_err(|error| error.to_string())?,
        b"1"
    );
    std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn macos_resume_clears_the_legacy_marker_without_claiming_admission() -> Result<(), String> {
    let state = disconnect_state_path();
    std::fs::create_dir_all(&state).map_err(|error| error.to_string())?;
    std::fs::write(state.join("drain"), b"1").map_err(|error| error.to_string())?;
    assert_eq!(
        resume_for_os(&state, Path::new("/unused"), "macos"),
        ExitCode::from(1)
    );
    assert!(!state.join("drain").exists());
    std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}
