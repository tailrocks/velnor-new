use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_STATE: AtomicUsize = AtomicUsize::new(0);

fn disconnect_state_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-disconnect-test-{}-{}",
        std::process::id(),
        NEXT_STATE.fetch_add(1, Ordering::Relaxed)
    ))
}
use std::process::ExitCode;

use super::{
    LocalBindingRemoval, disconnect_for_os, linux_disconnect_with, remove_local_binding_with,
    requested_wait,
};

#[test]
fn disconnect_requires_explicit_drain_and_wait() {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(
            &state,
            Path::new("/unused"),
            false,
            false,
            None,
            false,
            "macos"
        ),
        ExitCode::from(2)
    );
    assert!(!state.exists());
}

#[test]
fn macos_disconnect_keeps_legacy_marker_behavior() -> Result<(), String> {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(
            &state,
            Path::new("/unused"),
            true,
            true,
            Some(30),
            false,
            "macos"
        ),
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
fn macos_does_not_remove_local_binding_when_linux_only_option_is_requested() {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(
            &state,
            Path::new("/unused"),
            true,
            true,
            None,
            true,
            "macos"
        ),
        ExitCode::from(1)
    );
    assert!(!state.exists());
}

#[test]
fn successful_disconnect_stops_first_then_removes_only_requested_local_binding() {
    use std::cell::RefCell;

    let state = disconnect_state_path();
    std::fs::create_dir_all(&state).expect("state directory");
    std::fs::write(state.join("launch.db"), b"journal sentinel").expect("journal sentinel");
    let events = RefCell::new(Vec::new());
    let local = LocalBindingRemoval {
        config_text: "config bytes".to_owned(),
        credential_ref: "systemd-credential:github-token".to_owned(),
    };
    let result = linux_disconnect_with(
        Some(45),
        Some(local.clone()),
        |timeout| {
            assert_eq!(timeout, Some(45));
            events.borrow_mut().push("drain-stop-verify");
            ExitCode::SUCCESS
        },
        |binding| {
            assert_eq!(binding, &local);
            events.borrow_mut().push("remove-local-config-credential");
            Ok(())
        },
    );

    assert_eq!(result, ExitCode::SUCCESS);
    assert_eq!(
        events.into_inner(),
        ["drain-stop-verify", "remove-local-config-credential"]
    );
    assert_eq!(
        std::fs::read(state.join("launch.db")).expect("journal retained"),
        b"journal sentinel"
    );
    assert_eq!(
        velnor_runner_host::disconnect_effects(velnor_runner_host::SetOwnership::Adopted, true),
        vec![velnor_runner_host::DisconnectEffect::Drain]
    );
    std::fs::remove_dir_all(state).expect("test cleanup");
}

#[test]
fn successful_disconnect_retains_config_credential_and_state_by_default() {
    use std::cell::Cell;

    let remove_called = Cell::new(false);
    let result = linux_disconnect_with(
        None,
        None::<LocalBindingRemoval>,
        |timeout| {
            assert_eq!(timeout, None);
            ExitCode::SUCCESS
        },
        |_| {
            remove_called.set(true);
            Ok(())
        },
    );
    assert_eq!(result, ExitCode::SUCCESS);
    assert!(!remove_called.get());
}

#[test]
fn local_removal_deletes_config_before_credential() {
    use std::cell::RefCell;

    let events = RefCell::new(Vec::new());
    let result = remove_local_binding_with(
        || {
            events.borrow_mut().push("config");
            Ok(())
        },
        || {
            events.borrow_mut().push("credential");
            Ok(())
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(events.into_inner(), ["config", "credential"]);
}

#[test]
fn changed_config_keeps_its_credential() {
    use std::cell::Cell;

    let credential_removed = Cell::new(false);
    let result = remove_local_binding_with(
        || Err(()),
        || {
            credential_removed.set(true);
            Ok(())
        },
    );

    assert_eq!(result, Err(()));
    assert!(!credential_removed.get());
}

#[test]
fn drain_stop_or_local_removal_failure_is_nonzero_and_never_claims_success() {
    use std::cell::Cell;

    let remove_called = Cell::new(false);
    let stopped = linux_disconnect_with(
        None,
        Some(()),
        |_| ExitCode::from(1),
        |()| {
            remove_called.set(true);
            Ok(())
        },
    );
    assert_eq!(stopped, ExitCode::from(1));
    assert!(!remove_called.get());

    let removal = linux_disconnect_with(None, Some(()), |_| ExitCode::SUCCESS, |()| Err(()));
    assert_eq!(removal, ExitCode::from(1));
}
