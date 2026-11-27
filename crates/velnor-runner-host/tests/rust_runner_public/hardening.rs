// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::support::{Fixture, GUEST, SECRET_JIT, jit, reservation, wait_for};

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
    launcher.ensure_machine()?;
    let mut job = launcher.start_job(reservation(41)?, &jit(SECRET_JIT))?;
    assert!(job.wait().await?.success());
    fixture.configure("FAKE_RUNNER_HOLD=1\n")?;
    let held = launcher.start_job(reservation(42)?, &jit(SECRET_JIT))?;
    wait_for(&fixture.seen("42.jit.pid")).await?;
    held.stop().await?;
    std::fs::write(fixture.root.join("ps.table"), "4242 sleep 600\n")?;
    assert_eq!(launcher.list_processes()?.len(), 2);
    launcher.kill_processes(&[4242])?;

    let log = fixture.log()?;
    for expected in [
        "list -f json".to_owned(),
        "create --isolated --isolate-network --disk 172G --mount ".to_owned(),
        format!("{GUEST} install -d -o root -g root -m 0755"),
        format!("{GUEST} sha256sum /var/lib/velnor/bin/.stage"),
        "/var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor-host/jit/41.jit".to_owned(),
        format!("{GUEST} ps -eo pid=,args="),
        format!("{GUEST} kill -9 4242"),
    ] {
        assert!(
            log.iter().any(|line| line.contains(&expected)),
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
fn rust_runner_jit_directory_is_private() -> Result<(), Box<dyn Error>> {
    const TEST: &str = "hardening::rust_runner_jit_directory_is_private";
    if !is_child_of(TEST) {
        rerun(TEST, "umask 0;", &[])?;
        return Ok(());
    }
    let fresh = Fixture::ready()?;
    fresh.launcher().ensure_machine()?;
    assert_eq!(mode_of(&fresh.store.join("jit"))?, 0o700);
    assert_eq!(mode_of(&fresh.store.join("trusted"))?, 0o700);

    for name in ["jit", "trusted"] {
        for mode in [0o755, 0o750, 0o710, 0o770, 0o701, 0o777, 0o000] {
            let fixture = Fixture::ready()?;
            let directory = fixture.store.join(name);
            std::fs::create_dir_all(&directory)?;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(mode))?;
            fixture.launcher().ensure_machine()?;
            assert_eq!(mode_of(&directory)?, 0o700, "{name} {mode:o}");
        }
    }
    Ok(())
}

#[test]
fn rust_runner_recreates_a_share_directory_that_was_swapped_for_a_link_or_a_file() -> Check {
    for name in ["jit", "trusted"] {
        let linked = Fixture::ready()?;
        let victim = linked.root.join("victim");
        std::fs::create_dir(&victim)?;
        std::fs::write(victim.join("keep"), "")?;
        std::os::unix::fs::symlink(&victim, linked.store.join(name))?;
        linked.launcher().ensure_machine()?;
        let made = linked.store.join(name);
        assert!(!std::fs::symlink_metadata(&made)?.file_type().is_symlink());
        assert_eq!(mode_of(&made)?, 0o700, "{name}");
        assert!(
            victim.join("keep").exists(),
            "{name}: the link target changed"
        );

        let filed = Fixture::ready()?;
        std::fs::write(filed.store.join(name), "not a directory")?;
        filed.launcher().ensure_machine()?;
        assert!(filed.store.join(name).is_dir(), "{name}");
    }
    Ok(())
}

type Check = Result<(), Box<dyn Error>>;

fn mode_of(path: &Path) -> Result<u32, Box<dyn Error>> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o7777)
}
