//! Per-user `LaunchAgent` install. The daemon stays in the foreground.

use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::process::{Command, ExitCode};

#[cfg(target_os = "macos")]
use velnor_runner_host::launch_agent_plist;

use crate::args::ServiceAction;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(test)]
mod tests;

#[cfg(any(target_os = "macos", test))]
const LABEL: &str = "com.tailrocks.velnor.host";

/// Read-only view of whether the platform may still own the controller.
/// Unknown is never treated as stopped by mutating commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControllerServiceState {
    /// The controller unit/job is loaded, running, or transitioning.
    InUse,
    /// The platform positively reported the controller absent or inactive.
    Stopped,
    /// The manager could not provide a complete, trustworthy observation.
    Unknown,
}

pub(crate) fn service(action: ServiceAction, config_path: &Path, state_path: &Path) -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        linux::service(action, config_path, state_path)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (config_path, state_path);
        service_macos(action)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (action, config_path, state_path);
        eprintln!("service management unsupported on this host");
        ExitCode::from(1)
    }
}

pub(crate) const fn controller_service_status_line(
    state: ControllerServiceState,
) -> Option<&'static str> {
    match state {
        ControllerServiceState::InUse => Some("controller_service=in_use"),
        ControllerServiceState::Stopped => Some("controller_service=stopped_or_absent"),
        ControllerServiceState::Unknown => None,
    }
}

#[cfg(target_os = "macos")]
fn print_controller_service_status(state: ControllerServiceState) -> ExitCode {
    if let Some(line) = controller_service_status_line(state) {
        println!("{line}");
        ExitCode::SUCCESS
    } else {
        eprintln!("controller_service=unknown");
        ExitCode::from(1)
    }
}

#[cfg(target_os = "macos")]
fn service_macos(action: ServiceAction) -> ExitCode {
    match action {
        ServiceAction::Status => print_controller_service_status(controller_service_state()),
        ServiceAction::Install => install(),
        ServiceAction::Start => start(),
        ServiceAction::Stop => stop(),
        ServiceAction::Uninstall => uninstall(),
    }
}

pub(crate) fn logs(follow: bool) -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        command_code("journalctl", &journalctl_args(follow))
    }
    #[cfg(target_os = "macos")]
    {
        let path = log_dir().join("host.log");
        if !path.is_file() {
            eprintln!("log missing");
            return ExitCode::from(1);
        }
        let mut command = Command::new("tail");
        command.arg("-n").arg("200");
        if follow {
            command.arg("-F");
        }
        match command.arg(path).status() {
            Ok(status) if status.success() => ExitCode::SUCCESS,
            _ => ExitCode::from(1),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = follow;
        eprintln!("log backend unsupported on this host");
        ExitCode::from(1)
    }
}

/// Query the platform service manager without treating errors as stopped.
pub(crate) fn controller_service_state() -> ControllerServiceState {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("systemctl")
            .args([
                "show",
                "--no-pager",
                "--property=LoadState,ActiveState,SubState,MainPID,ControlPID",
                "velnor-host.service",
            ])
            .output();
        match output {
            Ok(output) => systemd_service_state(output.status.success(), &output.stdout),
            Err(_) => ControllerServiceState::Unknown,
        }
    }
    #[cfg(target_os = "macos")]
    {
        if read_uid().is_none() {
            return ControllerServiceState::Unknown;
        }
        match Command::new("launchctl").arg("list").output() {
            Ok(output) => launchctl_service_state(output.status.success(), &output.stdout, LABEL),
            Err(_) => ControllerServiceState::Unknown,
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        ControllerServiceState::Unknown
    }
}

fn systemd_service_state(success: bool, output: &[u8]) -> ControllerServiceState {
    if !success {
        return ControllerServiceState::Unknown;
    }
    let Ok(output) = std::str::from_utf8(output) else {
        return ControllerServiceState::Unknown;
    };
    let mut load = None;
    let mut active = None;
    let mut sub = None;
    let mut main_pid = None;
    let mut control_pid = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            return ControllerServiceState::Unknown;
        };
        let target = match key {
            "LoadState" => &mut load,
            "ActiveState" => &mut active,
            "SubState" => &mut sub,
            "MainPID" => &mut main_pid,
            "ControlPID" => &mut control_pid,
            _ => continue,
        };
        if target.replace(value).is_some() {
            return ControllerServiceState::Unknown;
        }
    }
    let (Some(load), Some(active), Some(sub), Some(main_pid), Some(control_pid)) =
        (load, active, sub, main_pid, control_pid)
    else {
        return ControllerServiceState::Unknown;
    };
    let (Ok(main_pid), Ok(control_pid)) = (main_pid.parse::<u32>(), control_pid.parse::<u32>())
    else {
        return ControllerServiceState::Unknown;
    };
    if main_pid != 0 || control_pid != 0 {
        return ControllerServiceState::InUse;
    }
    match (load, active, sub) {
        ("loaded" | "not-found", "inactive", "dead") => ControllerServiceState::Stopped,
        ("loaded", "active" | "activating" | "deactivating" | "reloading", _) => {
            ControllerServiceState::InUse
        }
        _ => ControllerServiceState::Unknown,
    }
}

#[cfg(any(target_os = "macos", test))]
fn launchctl_service_state(success: bool, output: &[u8], label: &str) -> ControllerServiceState {
    if !success {
        return ControllerServiceState::Unknown;
    }
    let Ok(output) = std::str::from_utf8(output) else {
        return ControllerServiceState::Unknown;
    };
    let mut saw_header = false;
    let mut saw_row = false;
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields == ["PID", "Status", "Label"] {
            if saw_header || saw_row {
                return ControllerServiceState::Unknown;
            }
            saw_header = true;
            continue;
        }
        if fields.len() != 3
            || (fields[0] != "-" && fields[0].parse::<u32>().is_err())
            || fields[1].parse::<i32>().is_err()
        {
            return ControllerServiceState::Unknown;
        }
        saw_row = true;
        if fields[2] == label {
            return ControllerServiceState::InUse;
        }
    }
    if saw_header || saw_row {
        ControllerServiceState::Stopped
    } else {
        ControllerServiceState::Unknown
    }
}

#[cfg(target_os = "linux")]
fn journalctl_args(follow: bool) -> Vec<&'static str> {
    let mut args = vec!["--unit=velnor-host.service", "--lines=200", "--no-pager"];
    if follow {
        args.push("--follow");
    }
    args
}

#[cfg(target_os = "linux")]
fn command_code(program: &str, args: &[&str]) -> ExitCode {
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        _ => ExitCode::from(1),
    }
}

#[cfg(target_os = "macos")]
fn start() -> ExitCode {
    let Some(uid) = read_uid() else {
        return ExitCode::from(1);
    };
    if std::fs::create_dir_all(log_dir()).is_err() {
        return ExitCode::from(1);
    }
    spawn(&bootstrap_argv(uid, &plist_path()))
}

#[cfg(target_os = "macos")]
fn stop() -> ExitCode {
    let Some(uid) = read_uid() else {
        return ExitCode::from(1);
    };
    spawn(&bootout_argv(uid))
}

#[cfg(target_os = "macos")]
fn uninstall() -> ExitCode {
    let _stopped = stop();
    match std::fs::remove_file(plist_path()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}

#[cfg(target_os = "macos")]
fn install() -> ExitCode {
    let Ok(bin) = std::env::current_exe() else {
        return ExitCode::from(1);
    };
    let Ok(plist) = launch_agent_plist(&bin) else {
        return ExitCode::from(1);
    };
    let path = plist_path();
    let Some(parent) = path.parent() else {
        return ExitCode::from(1);
    };
    if std::fs::create_dir_all(parent).is_err() || std::fs::create_dir_all(log_dir()).is_err() {
        return ExitCode::from(1);
    }
    let body = with_logs(&plist, &log_dir());
    if std::fs::write(&path, body).is_err() {
        return ExitCode::from(1);
    }
    println!("{}", path.display());
    ExitCode::SUCCESS
}

#[cfg(target_os = "macos")]
fn spawn(argv: &[String]) -> ExitCode {
    let Some((bin, rest)) = argv.split_first() else {
        return ExitCode::from(1);
    };
    if bin.is_empty() {
        return ExitCode::from(1);
    }
    match Command::new(bin).args(rest).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        _ => ExitCode::from(1),
    }
}

#[cfg(target_os = "macos")]
fn read_uid() -> Option<u32> {
    let output = Command::new("id").arg("-u").output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let uid = text.trim().parse().ok()?;
    if uid == 0 { None } else { Some(uid) }
}

#[cfg(target_os = "macos")]
pub(crate) fn log_dir() -> PathBuf {
    home().join("Library/Logs/Velnor")
}

#[cfg(target_os = "macos")]
fn plist_path() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

#[cfg(target_os = "macos")]
fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

#[cfg(any(target_os = "macos", test))]
fn bootstrap_argv(uid: u32, plist: &Path) -> Vec<String> {
    vec![
        "launchctl".to_owned(),
        "bootstrap".to_owned(),
        format!("gui/{uid}"),
        plist.display().to_string(),
    ]
}

#[cfg(any(target_os = "macos", test))]
fn bootout_argv(uid: u32) -> Vec<String> {
    vec![
        "launchctl".to_owned(),
        "bootout".to_owned(),
        format!("gui/{uid}/{LABEL}"),
    ]
}

#[cfg(any(target_os = "macos", test))]
fn with_logs(plist: &str, log_dir: &Path) -> String {
    let out = log_dir.join("host.log");
    let err = log_dir.join("host.err.log");
    let keys = format!(
        "<key>StandardOutPath</key><string>{}</string>\n<key>StandardErrorPath</key><string>{}</string>\n</dict>",
        out.display(),
        err.display()
    );
    plist.replacen("</dict>", &keys, 1)
}
