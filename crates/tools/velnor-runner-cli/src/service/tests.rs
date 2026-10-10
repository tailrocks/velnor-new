use std::path::Path;

#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use std::{
    ffi::OsStr,
    process::{Command, ExitCode, Stdio},
    sync::mpsc,
    thread,
};

use super::{ControllerServiceState, controller_service_status_line, systemd_service_state};

#[cfg(target_os = "macos")]
use super::{bootout_argv, bootstrap_argv, launchctl_service_state, with_logs};

#[cfg(target_os = "linux")]
use super::journalctl_command;

#[cfg(target_os = "linux")]
use super::{JOURNALCTL_PATH, busctl_command, command_code, journalctl_args, systemctl_command};

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

#[cfg(target_os = "linux")]
#[test]
fn linux_logs_select_packaged_journalctl_with_exact_unit_arguments() {
    let command = journalctl_command(false);
    assert_eq!(command.get_program(), OsStr::new(JOURNALCTL_PATH));
    assert_eq!(JOURNALCTL_PATH, "/usr/bin/journalctl");
    assert_eq!(
        journalctl_args(false),
        vec!["--unit=velnor-host.service", "--lines=200", "--no-pager",]
    );
    assert_eq!(
        journalctl_command(true).get_args().collect::<Vec<_>>(),
        journalctl_args(true)
            .iter()
            .map(OsStr::new)
            .collect::<Vec<_>>()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_log_follower_streams_output_and_waits_for_injected_child()
-> Result<(), Box<dyn std::error::Error>> {
    let scratch = TestDir::new("velnor-log-follow")?;
    let fake = scratch.path().join("journal-reader");
    let capture = scratch.path().join("args.txt");
    let output = scratch.path().join("stream.txt");
    std::fs::write(
        &fake,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$VELNOR_TEST_CAPTURE\"\nprintf 'before-follow\\n'\n/bin/sleep 1\nprintf 'after-follow\\n'\n",
    )?;
    let mut permissions = std::fs::metadata(&fake)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake, permissions)?;

    let mut command = Command::new(&fake);
    command
        .args(journalctl_args(true))
        .env("VELNOR_TEST_CAPTURE", &capture)
        .stdout(Stdio::from(std::fs::File::create(&output)?))
        .stderr(Stdio::null());
    let (sender, receiver) = mpsc::channel();
    let follower = thread::spawn(move || {
        let status = command_code(command);
        assert!(
            sender.send(status).is_ok(),
            "journal follower test receiver went away"
        );
    });

    let first = wait_for_output(&output, "before-follow\n", Duration::from_secs(3))?;
    assert!(!first.contains("after-follow\n"));
    assert!(
        receiver.try_recv().is_err(),
        "follower returned before its child"
    );
    let complete = wait_for_output(&output, "after-follow\n", Duration::from_secs(3))?;
    assert!(complete.contains("before-follow\n"));
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(3))?,
        ExitCode::SUCCESS
    );
    follower
        .join()
        .map_err(|_| "journal follower thread panicked")?;

    assert_eq!(
        std::fs::read_to_string(capture)?
            .lines()
            .collect::<Vec<_>>(),
        journalctl_args(true)
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn wait_for_output(
    path: &Path,
    expected: &str,
    timeout: Duration,
) -> Result<String, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        if contents.contains(expected) {
            return Ok(contents);
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for streamed output {expected:?}").into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "linux")]
struct TestDir(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl TestDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{sequence}", std::process::id()));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(target_os = "linux")]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
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

#[cfg(target_os = "linux")]
#[test]
fn linux_service_status_requests_the_unit_job_property() {
    assert_eq!(
        super::SERVICE_STATE_PROPERTIES,
        "LoadState,ActiveState,SubState,MainPID,ControlPID,Job"
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
        b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\nJob=\n";
    assert_eq!(
        systemd_service_state(true, stopped),
        ControllerServiceState::Stopped
    );

    let transitional = b"LoadState=loaded\nActiveState=activating\nSubState=start\nMainPID=0\nControlPID=0\nJob=\n";
    assert_eq!(
        systemd_service_state(true, transitional),
        ControllerServiceState::InUse
    );

    let active_with_pid =
        b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=12\nControlPID=0\nJob=\n";
    assert_eq!(
        systemd_service_state(true, active_with_pid),
        ControllerServiceState::InUse
    );
}

#[test]
fn systemd_pending_job_prevents_a_stopped_report() {
    let pending_start = b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\nJob=42 /org/freedesktop/systemd1/job/42\n";
    assert_eq!(
        systemd_service_state(true, pending_start),
        ControllerServiceState::InUse
    );
}

#[test]
fn systemd_query_failures_and_incomplete_states_are_unknown() {
    let stopped =
        b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\nJob=\n";
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
            b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\n"
        ),
        ControllerServiceState::Unknown
    );
    assert_eq!(
        systemd_service_state(
            true,
            b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\nJob=\nJob=\n"
        ),
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
