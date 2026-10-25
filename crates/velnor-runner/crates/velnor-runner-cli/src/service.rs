//! Per-user `LaunchAgent` install. The daemon stays in the foreground.

mod filesystem;
mod plist;
mod quarantine;
mod staging;
#[cfg(test)]
mod tests;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::args::ServiceAction;
use filesystem::write_plist;
use plist::{plist_path, render_plist};
use quarantine::{open_expected_file, uninstall_after_stop};

#[cfg(test)]
use filesystem::{atomic_write, publish_staged};
pub(crate) use plist::log_dir;
#[cfg(test)]
use plist::{plist_matches, with_logs, with_state};
#[cfg(test)]
use quarantine::existing_matches;
#[cfg(test)]
use quarantine::same_file;
#[cfg(test)]
use quarantine::{path_is_absent, uninstall_after_stop_with};
#[cfg(test)]
use staging::PrivateDirectory;

const LABEL: &str = "com.tailrocks.velnor.host";
const LAUNCHCTL: &str = "/bin/launchctl";

struct InstalledPlist {
    uid: u32,
    path: PathBuf,
    expected: String,
    identity: File,
}

pub(crate) fn service(action: ServiceAction, state: &Path) -> ExitCode {
    match action {
        ServiceAction::Install => install(state),
        ServiceAction::Start => start(state),
        ServiceAction::Stop => stop(state),
        ServiceAction::Uninstall => uninstall(state),
    }
}

fn start(state: &Path) -> ExitCode {
    if std::fs::create_dir_all(log_dir()).is_err() {
        eprintln!("Service configuration is different.");
        return ExitCode::from(1);
    }
    let Some(installed) = installed_plist(state) else {
        eprintln!("Service configuration is different.");
        return ExitCode::from(1);
    };
    let result = spawn(&bootstrap_argv(installed.uid, &installed.path));
    drop(installed);
    result
}

fn stop(state: &Path) -> ExitCode {
    let Some(installed) = installed_plist(state) else {
        eprintln!("Service configuration is different.");
        return ExitCode::from(1);
    };
    let result = spawn(&bootout_argv(installed.uid));
    drop(installed);
    result
}

fn uninstall(state: &Path) -> ExitCode {
    let Some(installed) = installed_plist(state) else {
        eprintln!("Service configuration is different.");
        return ExitCode::from(1);
    };
    let stopped = spawn(&bootout_argv(installed.uid));
    uninstall_after_stop(
        stopped,
        &installed.path,
        &installed.identity,
        installed.expected.as_bytes(),
    )
}

fn installed_plist(state: &Path) -> Option<InstalledPlist> {
    let uid = read_uid()?;
    let binary = std::env::current_exe().ok()?;
    let path = plist_path();
    let expected = render_plist(&binary, state, &log_dir())?;
    let identity = open_expected_file(&path, expected.as_bytes()).ok()??;
    Some(InstalledPlist {
        uid,
        path,
        expected,
        identity,
    })
}

fn install(state: &Path) -> ExitCode {
    let Ok(bin) = std::env::current_exe() else {
        return ExitCode::from(1);
    };
    let path = plist_path();
    if !write_plist(&bin, state, &path, &log_dir()) {
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
    let uid = rustix::process::geteuid().as_raw();
    if uid == 0 { None } else { Some(uid) }
}

fn bootstrap_argv(uid: u32, plist: &Path) -> Vec<String> {
    vec![
        LAUNCHCTL.to_owned(),
        "bootstrap".to_owned(),
        format!("gui/{uid}"),
        plist.display().to_string(),
    ]
}

fn bootout_argv(uid: u32) -> Vec<String> {
    vec![
        LAUNCHCTL.to_owned(),
        "bootout".to_owned(),
        format!("gui/{uid}/{LABEL}"),
    ]
}
