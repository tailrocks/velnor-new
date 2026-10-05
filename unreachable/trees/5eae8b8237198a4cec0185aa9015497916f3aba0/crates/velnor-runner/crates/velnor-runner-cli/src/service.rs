//! Per-user `LaunchAgent` install. The daemon stays in the foreground.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use velnor_runner_host::launch_agent_plist;

use crate::args::ServiceAction;

const LABEL: &str = "com.tailrocks.velnor.host";

pub(crate) fn service(action: ServiceAction) -> ExitCode {
    match action {
        ServiceAction::Install => install(),
        ServiceAction::Start => start(),
        ServiceAction::Stop => stop(),
        ServiceAction::Uninstall => uninstall(),
    }
}

fn start() -> ExitCode {
    let Some(uid) = read_uid() else {
        return ExitCode::from(1);
    };
    if std::fs::create_dir_all(log_dir()).is_err() {
        return ExitCode::from(1);
    }
    spawn(&bootstrap_argv(uid, &plist_path()))
}

fn stop() -> ExitCode {
    let Some(uid) = read_uid() else {
        return ExitCode::from(1);
    };
    spawn(&bootout_argv(uid))
}

fn uninstall() -> ExitCode {
    let _stopped = stop();
    match std::fs::remove_file(plist_path()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}

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

fn read_uid() -> Option<u32> {
    let output = Command::new("id").arg("-u").output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let uid = text.trim().parse().ok()?;
    if uid == 0 { None } else { Some(uid) }
}

pub(crate) fn log_dir() -> PathBuf {
    home().join("Library/Logs/Velnor")
}

fn plist_path() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn bootstrap_argv(uid: u32, plist: &Path) -> Vec<String> {
    vec![
        "launchctl".to_owned(),
        "bootstrap".to_owned(),
        format!("gui/{uid}"),
        plist.display().to_string(),
    ]
}

fn bootout_argv(uid: u32) -> Vec<String> {
    vec![
        "launchctl".to_owned(),
        "bootout".to_owned(),
        format!("gui/{uid}/{LABEL}"),
    ]
}

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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{bootout_argv, bootstrap_argv, with_logs};

    #[test]
    fn launchctl_uses_the_gui_domain_and_does_not_fork() {
        let plist =
            Path::new("/Users/example/Library/LaunchAgents/com.tailrocks.velnor.host.plist");
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
}
