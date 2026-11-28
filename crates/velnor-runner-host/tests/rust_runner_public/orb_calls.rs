// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::time::{Duration, Instant};

use velnor_runner_host::{HostError, RustRunnerFailure};

use super::support::{Fixture, process_exists};

const CHILD: &str = "VELNOR_TEST_ORB_CHILD";
const INCOMPATIBLE: &str = r#"[{"name":"velnor","image":{"distro":"debian","arch":"arm64"}}]"#;

fn grandchild(fixture: &Fixture) -> Result<String, Box<dyn Error>> {
    let found = std::fs::read_dir(fixture.root.join("seen"))?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("grandchild.")
        })
        .ok_or("the fake orb started no descendant")?;
    Ok(std::fs::read_to_string(found.path())?.trim().to_owned())
}

fn wait_until_dead(pid: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while process_exists(pid) {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

#[test]
fn rust_runner_orb_call_ends_the_descendants_it_leaves_holding_stdout() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::ready()?;
    fixture.configure("FAKE_ORB_GRANDCHILD=group\n")?;
    let launcher = fixture.launcher().with_orb_deadline(Duration::from_secs(8));
    let started = Instant::now();
    launcher.ensure_machine().map_err(|error| {
        format!(
            "an orb call that left a descendant holding stdout failed instead of ending it: {error}"
        )
    })?;
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "an orb call that left a descendant holding stdout waited {:?} for it",
        started.elapsed()
    );
    assert!(
        wait_until_dead(&grandchild(&fixture)?),
        "the descendant of a finished orb call survived"
    );
    Ok(())
}

#[test]
fn rust_runner_orb_call_bounds_the_stdout_read_by_its_deadline() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::ready()?;
    fixture.configure("FAKE_ORB_GRANDCHILD=escaped\n")?;
    let launcher = fixture.launcher().with_orb_deadline(Duration::from_secs(1));
    let started = Instant::now();
    let outcome = launcher.ensure_machine();
    let elapsed = started.elapsed();
    let escaped = grandchild(&fixture)?;
    std::process::Command::new("kill")
        .args(["-9", &escaped])
        .status()?;
    assert_eq!(
        outcome.err(),
        Some(HostError::RustRunner(RustRunnerFailure::OrbTimeout)),
        "an orb call whose stdout stayed open past the deadline must time out"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the stdout read was not bounded by the deadline: {elapsed:?}"
    );
    Ok(())
}

#[test]
fn rust_runner_runs_the_orb_named_by_velnor_orb() -> Result<(), Box<dyn Error>> {
    if std::env::var_os(CHILD).is_some() {
        let outcome = velnor_runner_host::RustRunnerLauncher::from_environment().ensure_machine();
        println!("OUTCOME={:?}", outcome.err());
        return Ok(());
    }
    let fixture = Fixture::new(INCOMPATIBLE)?;
    let bun = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("bun"))
        .find(|candidate| candidate.is_file())
        .ok_or("bun is not on PATH")?;
    let tools = fixture.root.join("tools");
    std::fs::create_dir(&tools)?;
    std::os::unix::fs::symlink(bun, tools.join("bun"))?;
    let path = std::env::join_paths([tools, "/usr/bin".into(), "/bin".into()])?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "orb_calls::rust_runner_runs_the_orb_named_by_velnor_orb",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("VELNOR_ORB", fixture.root.join("orb"))
        .env("PATH", path)
        .output()?;
    let reported = String::from_utf8(output.stdout)?;
    assert!(output.status.success(), "{reported}");
    let expected = format!(
        "OUTCOME={:?}",
        Some(HostError::RustRunner(
            RustRunnerFailure::MachineIncompatible
        ))
    );
    assert!(reported.lines().any(|line| line == expected), "{reported}");
    assert_eq!(fixture.log()?, ["list -f json"]);
    Ok(())
}
