use std::path::Path;

#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    ControllerServiceState, bootout_argv, bootstrap_argv, controller_service_status_line,
    launchctl_service_state, systemd_service_state, with_logs,
};

#[cfg(target_os = "linux")]
use super::{busctl_command, systemctl_command};

#[cfg(target_os = "linux")]
static NEXT_SYSTEMD_SHIM: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "linux")]
#[test]
fn linux_systemd_tools_ignore_path_shims() -> Result<(), Box<dyn std::error::Error>> {
    let shim_dir = std::env::temp_dir().join(format!(
        "velnor-systemd-tool-path-{}-{}",
        std::process::id(),
        NEXT_SYSTEMD_SHIM.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&shim_dir)?;
    let marker = shim_dir.join("path-shim-used");
    let script = "#!/bin/sh\n: > \"$VELNOR_SYSTEMD_SHIM_MARKER\"\nexit 0\n";
    for name in ["systemctl", "busctl"] {
        let shim = shim_dir.join(name);
        std::fs::write(&shim, script)?;
        let mut permissions = std::fs::metadata(&shim)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions)?;
    }

    for mut command in [systemctl_command(), busctl_command()] {
        let output = command
            .arg("--version")
            .env("PATH", &shim_dir)
            .env("VELNOR_SYSTEMD_SHIM_MARKER", &marker)
            .output()?;
        assert!(output.status.success());
        assert!(!marker.exists(), "the PATH shim must not be executed");
    }

    std::fs::remove_dir_all(shim_dir)?;
    Ok(())
}

#[test]
fn controller_service_status_is_distinct_from_readiness() {
    assert_eq!(
        controller_service_status_line(ControllerServiceState::InUse),
        Some("controller_service=in_use")
    );
    assert_eq!(
        controller_service_status_line(ControllerServiceState::Stopped),
        Some("controller_service=stopped_or_absent")
    );
    assert_eq!(
        controller_service_status_line(ControllerServiceState::Unknown),
        None
    );
}

#[test]
fn launchctl_uses_the_gui_domain_and_does_not_fork() {
    let plist = Path::new("/Users/example/Library/LaunchAgents/com.tailrocks.velnor.host.plist");
    assert_eq!(
        bootstrap_argv(501, plist),
        vec![
            "launchctl".to_owned(),
            "bootstrap".to_owned(),
            "gui/501".to_owned(),
            plist.display().to_string(),
        ]
    );
    assert_eq!(
        bootout_argv(501),
        vec![
            "launchctl".to_owned(),
            "bootout".to_owned(),
            "gui/501/com.tailrocks.velnor.host".to_owned(),
        ]
    );
}

#[test]
fn plist_logs_stay_out_of_the_secret_channel() {
    let body = with_logs(
        "<dict>\n</dict>\n",
        Path::new("/Users/example/Library/Logs/Velnor"),
    );
    assert!(body.contains("StandardOutPath"));
    assert!(body.contains("/Users/example/Library/Logs/Velnor/host.log"));
    assert!(!body.contains("ACTIONS_RUNNER_INPUT_JITCONFIG"));
    assert!(!body.contains("daemon fork"));
}

#[test]
fn systemd_requires_a_complete_positive_stopped_observation() {
    let stopped =
        b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\n";
    assert_eq!(
        systemd_service_state(true, stopped),
        ControllerServiceState::Stopped
    );

    let transitional =
        b"LoadState=loaded\nActiveState=activating\nSubState=start\nMainPID=0\nControlPID=0\n";
    assert_eq!(
        systemd_service_state(true, transitional),
        ControllerServiceState::InUse
    );

    let active_with_pid =
        b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=12\nControlPID=0\n";
    assert_eq!(
        systemd_service_state(true, active_with_pid),
        ControllerServiceState::InUse
    );
}

#[test]
fn systemd_query_failures_and_incomplete_states_are_unknown() {
    let stopped =
        b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\n";
    assert_eq!(
        systemd_service_state(false, stopped),
        ControllerServiceState::Unknown
    );
    assert_eq!(
        systemd_service_state(true, b"LoadState=loaded\nActiveState=inactive\n"),
        ControllerServiceState::Unknown
    );
    assert_eq!(
        systemd_service_state(
            true,
            b"LoadState=loaded\nActiveState=failed\nSubState=failed\nMainPID=0\nControlPID=0\n"
        ),
        ControllerServiceState::Unknown
    );
}

#[test]
fn launchctl_requires_success_and_parses_the_exact_label() {
    assert_eq!(
        launchctl_service_state(
            true,
            b"PID Status Label\n501 0 com.tailrocks.velnor.host\n",
            "com.tailrocks.velnor.host"
        ),
        ControllerServiceState::InUse
    );
    assert_eq!(
        launchctl_service_state(
            true,
            b"PID Status Label\n- 0 com.example.other\n",
            "com.tailrocks.velnor.host"
        ),
        ControllerServiceState::Stopped
    );
    assert_eq!(
        launchctl_service_state(true, b"PID Status Label\n", "com.tailrocks.velnor.host"),
        ControllerServiceState::Stopped
    );
    assert_eq!(
        launchctl_service_state(false, b"", "com.tailrocks.velnor.host"),
        ControllerServiceState::Unknown
    );
    assert_eq!(
        launchctl_service_state(true, b"unexpected output\n", "com.tailrocks.velnor.host"),
        ControllerServiceState::Unknown
    );
    assert_eq!(
        launchctl_service_state(
            true,
            b"PID Status Label\nPID Status Label\n",
            "com.tailrocks.velnor.host"
        ),
        ControllerServiceState::Unknown
    );
}
