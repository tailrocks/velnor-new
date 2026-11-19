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
use crate::state_directory::create_private_dir_all;
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
        ServiceAction::Start => start(state, &log_dir()),
        ServiceAction::Stop => stop(state),
        ServiceAction::Uninstall => uninstall(state),
    }
}

fn start(state: &Path, logs: &Path) -> ExitCode {
    if create_private_dir_all(logs).is_err() {
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
    install_to(
        rustix::process::geteuid().as_raw(),
        state,
        &plist_path(),
        &log_dir(),
    )
}

fn install_to(euid: u32, state: &Path, path: &Path, logs: &Path) -> ExitCode {
    if euid == 0 {
        eprintln!("Service configuration is different.");
        return ExitCode::from(1);
    }
    let Ok(bin) = std::env::current_exe() else {
        return ExitCode::from(1);
    };
    if !write_plist(&bin, state, path, logs) {
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::{install_to, start};

    fn mode(path: &Path) -> std::io::Result<u32> {
        Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
    }

    #[test]
    fn start_creates_a_missing_log_folder_private_and_keeps_its_parent_mode() -> std::io::Result<()>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let parent = root.join("parent");
        std::fs::create_dir(&parent)?;
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755))?;
        let logs = parent.join("Logs").join("Velnor");
        let _ = start(&root.join("state"), &logs);
        assert_eq!(mode(&logs)?, 0o700);
        assert_eq!(mode(&parent)?, 0o755);
        Ok(())
    }

    #[test]
    fn install_refuses_root_and_writes_no_plist() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let plist = root.join("LaunchAgents").join("host.plist");
        let code = install_to(0, &root.join("state"), &plist, &root.join("logs"));
        assert_eq!(
            format!("{code:?}"),
            format!("{:?}", std::process::ExitCode::from(1))
        );
        assert!(!plist.exists());
        assert!(!root.join("LaunchAgents").exists());
        Ok(())
    }
}
