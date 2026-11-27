// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use velnor_runner_host::{HostError, RustRunnerFailure};

use super::support::{Fixture, SECRET_JIT, facts, jit, reservation, wait_for};

const CHILD_MARKER: &str = "VELNOR_RUST_RUNNER_PUBLIC_CHILD";

fn rerun(
    test: &str,
    shell_prefix: &str,
    environment: &[(&str, &str)],
) -> Result<(), Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{shell_prefix} exec \"$0\" --exact {test} --nocapture"
        ))
        .arg(&executable)
        .env(CHILD_MARKER, test)
        .envs(environment.iter().copied())
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("running 1 test"), "{stdout}\n{stderr}");
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "{stdout}\n{stderr}"
    );
    Ok(())
}

fn is_child_of(test: &str) -> bool {
    std::env::var(CHILD_MARKER).is_ok_and(|marker| marker == test)
}

fn environment_keys_of_a_clean_shell() -> Result<Vec<String>, Box<dyn Error>> {
    let output = std::process::Command::new("env")
        .args(["-i", "sh", "-c", "env"])
        .output()?;
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key.to_owned()))
        .collect())
}

fn environment_of_call(path: &Path) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .filter_map(|line| {
            line.split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
        })
        .collect())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_orb_calls_clear_the_environment_and_never_set_orbenv()
-> Result<(), Box<dyn Error>> {
    const TEST: &str =
        "hardening::rust_runner_orb_calls_clear_the_environment_and_never_set_orbenv";
    if !is_child_of(TEST) {
        let path = format!(
            "{}:/rust-runner-l1-extra",
            std::env::var("PATH").unwrap_or_default()
        );
        return rerun(
            TEST,
            "",
            &[
                ("ORBENV", "PAT-CANARY-L1-ORBENV"),
                ("GH_TOKEN", "PAT-CANARY-L1-GHTOKEN"),
                ("VELNOR_SECRET", "PAT-CANARY-L1-SECRET"),
                ("HOME", "/rust-runner-l1-home"),
                ("PATH", &path),
            ],
        );
    }
    let fixture = Fixture::new("[]")?;
    let launcher = fixture.launcher();
    let user = launcher.ensure_machine()?;
    let mut job = launcher.start_job(reservation(41)?, &jit(SECRET_JIT), &facts(), &user)?;
    assert!(job.wait().await?.success());
    fixture.configure("FAKE_RUNNER_HOLD=1\n")?;
    let held = launcher.start_job(reservation(42)?, &jit(SECRET_JIT), &facts(), &user)?;
    wait_for(&fixture.seen("42.jit.pid")).await?;
    held.stop().await?;
    std::fs::write(fixture.root.join("ps.table"), "4242 sleep 600\n")?;
    assert_eq!(launcher.list_processes()?.len(), 2);
    launcher.kill_processes(&[4242])?;

    let log = fixture.log()?;
    for expected in [
        "list -f json",
        "create -a arm64 ubuntu velnor",
        "-m velnor -u root ln -sfn",
        "-m velnor test -x /var/lib/velnor/bin/velnor-runner",
        "--jit-stdin --trusted /var/lib/velnor-host/trusted/41.json -- /var/lib/velnor/bin/velnor-runner run --jit -",
        "-m velnor -u root ps -eo pid=,args=",
        "-m velnor -u root kill -9 4242",
    ] {
        assert!(
            log.iter().any(|line| line.contains(expected)),
            "{expected}: {log:?}"
        );
    }
    assert_environments_are_cleared(&fixture)
}

fn assert_environments_are_cleared(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let allowed = {
        let mut keys = environment_keys_of_a_clean_shell()?;
        keys.extend(["PATH".to_owned(), "HOME".to_owned()]);
        keys
    };
    let host_path = std::env::var("PATH")?;
    let host_home = std::env::var("HOME")?;
    let calls = std::fs::read_dir(fixture.seen("calls"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "env"))
        .collect::<Vec<_>>();
    assert!(calls.len() >= fixture.log()?.len(), "{}", calls.len());
    for call in calls {
        let environment = environment_of_call(&call)?;
        for (key, value) in &environment {
            assert!(
                allowed.contains(key),
                "{key} reached orb in {}",
                call.display()
            );
            assert_ne!(key, "ORBENV", "{}", call.display());
            assert!(!value.contains("PAT-CANARY"), "{key} in {}", call.display());
        }
        let value_of = |name: &str| {
            environment
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(
            value_of("PATH"),
            Some(host_path.as_str()),
            "{}",
            call.display()
        );
        assert_eq!(
            value_of("HOME"),
            Some(host_home.as_str()),
            "{}",
            call.display()
        );
    }
    Ok(())
}

#[test]
fn rust_runner_trusted_directory_is_private() -> Result<(), Box<dyn Error>> {
    const TEST: &str = "hardening::rust_runner_trusted_directory_is_private";
    if !is_child_of(TEST) {
        rerun(TEST, "umask 0;", &[])?;
        return Ok(());
    }
    let fresh = Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    fresh.launcher().ensure_machine()?;
    assert_eq!(mode_of(&fresh.store.join("trusted"))?, 0o700);

    for mode in [0o755, 0o750, 0o710, 0o770, 0o701, 0o777] {
        let fixture =
            Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
        std::fs::create_dir_all(fixture.store.join("trusted"))?;
        std::fs::set_permissions(
            fixture.store.join("trusted"),
            std::fs::Permissions::from_mode(mode),
        )?;
        assert_eq!(
            fixture.launcher().ensure_machine().err(),
            Some(HostError::RustRunner(
                RustRunnerFailure::TrustedDirectoryAccessible
            )),
            "{mode:o}"
        );
    }
    Ok(())
}

fn mode_of(path: &Path) -> Result<u32, Box<dyn Error>> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o7777)
}
