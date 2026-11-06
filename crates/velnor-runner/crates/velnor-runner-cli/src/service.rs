// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod filesystem;
mod plist;
mod quarantine;
mod staging;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::args::ServiceAction;
use filesystem::write_plist;
use plist::{plist_path, render_plist};
use quarantine::{open_expected_file, uninstall_after_stop};

pub(crate) use plist::log_dir;

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
    let stopped = spawn_status_code(&bootout_argv(installed.uid));
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
    if spawn_status_code(argv) == Some(0) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn spawn_status_code(argv: &[String]) -> Option<i32> {
    let (bin, rest) = argv.split_first()?;
    if bin.is_empty() {
        return None;
    }
    Command::new(bin).args(rest).status().ok()?.code()
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
