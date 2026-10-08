use std::path::PathBuf;
use std::process::ExitCode;

use super::{
    ConfigReadError, DaemonIntent, daemon_backend_supported, daemon_intent, read_daemon_config_with,
};
#[cfg(target_os = "linux")]
use super::{LinuxStartupFailure, run_linux_after_snapshot};
#[cfg(target_os = "linux")]
use velnor_runner_host::validate_protected_state_directory;
use velnor_runner_host::{DaemonLock, HostConfig, HostError, HostPlatform};

const SAMPLE: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"tailrocks/velnor-new\"\n",
    "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
    "credential_ref = \"keychain:com.tailrocks.velnor.host/local\"\n",
    "[host]\n",
    "max_jobs = 1\n",
    "[docker]\n",
    "context = \"orbstack\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

#[test]
fn credential_text_trims_and_rejects_empty() {
    assert_eq!(
        super::credential_text(b"canary-token\n"),
        Some("canary-token")
    );
    assert_eq!(super::credential_text(b"\n"), None);
    assert_eq!(super::credential_text(&[0xff, 0xfe]), None);
}

#[test]
fn missing_config_waits() {
    assert_eq!(daemon_intent(None), DaemonIntent::Wait);
}

#[test]
fn valid_host_toml_listens() {
    assert_eq!(daemon_intent(Some(SAMPLE)), DaemonIntent::Listen);
}

#[test]
fn daemon_reads_the_selected_config_file_not_state_host_toml() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("velnor-daemon-config-{}", std::process::id()));
    let state = dir.join("state");
    let selected = dir.join("operator-selected.toml");
    std::fs::create_dir_all(&state).map_err(|error| error.to_string())?;
    std::fs::write(state.join("host.toml"), b"not valid TOML\n")
        .map_err(|error| error.to_string())?;
    std::fs::write(&selected, SAMPLE).map_err(|error| error.to_string())?;

    let config = read_daemon_config_with(&selected, HostPlatform::Macos, |path, platform| {
        if platform != HostPlatform::Macos {
            return Err(());
        }
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(()),
        }
    })
    .map_err(|error| format!("selected config failed: {error:?}"))?
    .ok_or("selected config was treated as missing")?;
    if config.github.repository != "tailrocks/velnor-new" {
        return Err("daemon did not read the configured path".to_owned());
    }
    if !matches!(
        read_daemon_config_with(&state.join("host.toml"), HostPlatform::Macos, |path, _| {
            match std::fs::read_to_string(path) {
                Ok(text) => Ok(Some(text)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(()),
            }
        }),
        Err(ConfigReadError::Invalid)
    ) {
        return Err("fixture did not distinguish the legacy fallback path".to_owned());
    }
    std::fs::remove_dir_all(dir).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn daemon_backend_does_not_fall_back_to_the_legacy_launcher_on_linux() {
    assert!(!daemon_backend_supported(HostPlatform::Linux));
    assert!(daemon_backend_supported(HostPlatform::Macos));
}

#[test]
fn bad_toml_is_err_and_hides_the_secret() {
    let bad = SAMPLE.replace("schema = 1", "schema = 2\ntoken = \"secret\"");
    assert_eq!(daemon_intent(Some(&bad)), DaemonIntent::Err);
    let error = match HostConfig::parse(&bad) {
        Ok(_) => HostError::Config,
        Err(error) => error,
    };
    let text = format!("{error}");
    assert_eq!(text, "invalid config");
    assert!(!text.contains("secret"));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_startup_reads_config_before_lock_and_coordinator() {
    use std::cell::RefCell;

    let calls = RefCell::new(Vec::new());
    let result = run_linux_after_snapshot(
        || {
            calls.borrow_mut().push("snapshot");
            Ok(Some(()))
        },
        |()| {
            calls.borrow_mut().push("context");
            Ok(())
        },
        || {
            calls.borrow_mut().push("state");
            Ok(())
        },
        |()| {
            calls.borrow_mut().push("lock");
            Ok(())
        },
        |(), (), ()| {
            calls.borrow_mut().push("coordinator");
            ExitCode::SUCCESS
        },
    );

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    assert_eq!(
        *calls.borrow(),
        ["snapshot", "context", "state", "lock", "coordinator"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_startup_config_failure_or_missing_snapshot_never_acquires_lock() {
    use std::cell::Cell;

    for read_result in [Err(()), Ok(None)] {
        let lock_called = Cell::new(false);
        let state_called = Cell::new(false);
        let result = run_linux_after_snapshot(
            || read_result,
            |()| Ok(()),
            || {
                state_called.set(true);
                Ok(())
            },
            |()| {
                lock_called.set(true);
                Ok(())
            },
            |(), (), ()| ExitCode::SUCCESS,
        );

        assert_eq!(result, Err(LinuxStartupFailure::Configuration));
        assert!(!state_called.get());
        assert!(!lock_called.get());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_context_failure_does_not_probe_state_or_acquire_lock() {
    use std::cell::Cell;

    let state_called = Cell::new(false);
    let lock_called = Cell::new(false);
    let result = run_linux_after_snapshot(
        || Ok(Some(())),
        |()| Err(()),
        || {
            state_called.set(true);
            Ok(())
        },
        |()| {
            lock_called.set(true);
            Ok(())
        },
        |(), (), ()| ExitCode::SUCCESS,
    );

    assert_eq!(result, Err(LinuxStartupFailure::Context));
    assert!(!state_called.get());
    assert!(!lock_called.get());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_state_directory_protection_rejects_symlink_and_world_writable_paths_before_lock()
-> Result<(), String> {
    use std::cell::Cell;
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = std::env::temp_dir().join(format!("velnor-state-protection-{}", std::process::id()));
    remove_tree(&root);
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;

    let writable = root.join("writable-state");
    std::fs::create_dir(&writable).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&writable, std::fs::Permissions::from_mode(0o777))
        .map_err(|error| error.to_string())?;

    let target = root.join("safe-target");
    std::fs::create_dir(&target).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let linked = root.join("linked-state");
    symlink(&target, &linked).map_err(|error| error.to_string())?;

    for unsafe_state in [&writable, &linked] {
        let lock_called = Cell::new(false);
        let result = run_linux_after_snapshot(
            || Ok(Some(())),
            |()| Ok(()),
            || validate_protected_state_directory(unsafe_state).map_err(|_| ()),
            |_| {
                lock_called.set(true);
                Ok(())
            },
            |(), _, ()| ExitCode::SUCCESS,
        );
        assert_eq!(result, Err(LinuxStartupFailure::StateDirectory));
        assert!(!lock_called.get());
        assert!(!unsafe_state.join("daemon.lock").exists());
    }

    let safe = root.join("safe-state");
    std::fs::create_dir(&safe).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&safe, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let result = run_linux_after_snapshot(
        || Ok(Some(())),
        |()| Ok(()),
        || validate_protected_state_directory(&safe).map_err(|_| ()),
        |_| {
            assert!(!safe.join("diagnostics").exists());
            assert!(!safe.join("daemon.lock").exists());
            Ok(())
        },
        |(), state_directory, ()| {
            assert!(!safe.join("diagnostics").exists());
            assert!(!safe.join("daemon.lock").exists());
            assert!(std::fs::metadata(&safe).is_ok());
            drop(state_directory);
            ExitCode::SUCCESS
        },
    );
    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));

    remove_tree(&root);
    Ok(())
}

#[test]
fn second_daemon_lock_fails_closed() -> Result<(), String> {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("velnor-daemon-lock-{}", std::process::id()));
    remove_tree(&dir);
    std::fs::create_dir_all(&dir).map_err(|_| "mkdir".to_owned())?;
    let path = dir.join("daemon.lock");
    let first = DaemonLock::try_acquire(&path).map_err(|_| "first".to_owned())?;
    if !first.is_held() {
        return Err("not held".to_owned());
    }
    match DaemonLock::try_acquire(&path) {
        Err(HostError::Lock) => {}
        Ok(_) => return Err("second acquired".to_owned()),
        Err(_) => return Err("other".to_owned()),
    }
    drop(first);
    remove_tree(&dir);
    Ok(())
}

fn remove_tree(dir: &std::path::Path) {
    match std::fs::remove_dir_all(dir) {
        Ok(()) | Err(_) => {}
    }
}

mod daemon_linux_tests;
