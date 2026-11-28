// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::{Environment, Location, start_with, stop_with, write_plist};

struct Installed {
    location: Location,
    state: PathBuf,
    logs: PathBuf,
    orb: PathBuf,
}

fn install(root: &Path) -> std::io::Result<Installed> {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin)?;
    let tool = |name: &str| -> std::io::Result<PathBuf> {
        let path = bin.join(name);
        std::fs::write(&path, "#!/bin/sh\n")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        Ok(path)
    };
    let environment = Environment {
        orb: tool("orb")?,
        runner_binary: tool("velnor-runner")?,
    };
    let location = Location {
        uid: rustix::process::geteuid().as_raw(),
        binary: root.join("velnor-host"),
        plist: root.join("LaunchAgents").join("host.plist"),
    };
    let (state, logs) = (root.join("state"), root.join("logs"));
    assert!(write_plist(
        &location.binary,
        &environment,
        &state,
        &location.plist,
        &logs
    ));
    Ok(Installed {
        orb: environment.orb,
        location,
        state,
        logs,
    })
}

fn code(exit: ExitCode) -> String {
    format!("{exit:?}")
}

fn failure() -> String {
    code(ExitCode::from(1))
}

#[test]
fn start_and_stop_launch_a_service_whose_environment_is_still_trusted() -> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let installed = install(&temp.path().canonicalize()?)?;
    let calls = RefCell::new(Vec::new());
    let record = |argv: &[String]| {
        calls.borrow_mut().push(argv.join(" "));
        ExitCode::SUCCESS
    };
    let (state, logs) = (&installed.state, &installed.logs);
    assert_eq!(
        code(start_with(&installed.location, state, logs, record)),
        code(ExitCode::SUCCESS)
    );
    assert_eq!(
        code(stop_with(&installed.location, state, logs, record)),
        code(ExitCode::SUCCESS)
    );
    let calls = calls.into_inner();
    assert!(calls[0].contains("bootstrap"), "{calls:?}");
    assert!(calls[1].contains("bootout"), "{calls:?}");
    Ok(())
}

#[test]
fn start_and_stop_refuse_before_launchctl_when_the_recorded_orb_became_writable()
-> std::io::Result<()> {
    let temp = tempfile::tempdir()?;
    let installed = install(&temp.path().canonicalize()?)?;
    std::fs::set_permissions(&installed.orb, std::fs::Permissions::from_mode(0o775))?;
    let calls = RefCell::new(0_u32);
    let record = |_: &[String]| {
        *calls.borrow_mut() += 1;
        ExitCode::SUCCESS
    };
    let (state, logs) = (&installed.state, &installed.logs);
    assert_eq!(
        code(start_with(&installed.location, state, logs, record)),
        failure()
    );
    assert_eq!(
        code(stop_with(&installed.location, state, logs, record)),
        failure()
    );
    assert_eq!(*calls.borrow(), 0);
    Ok(())
}
