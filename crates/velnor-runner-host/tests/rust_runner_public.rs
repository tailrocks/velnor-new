// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "rust_runner_public/hardening.rs"]
mod hardening;
#[path = "rust_runner_public/inside.rs"]
mod inside;
#[path = "rust_runner_public/machine.rs"]
mod machine;
#[path = "rust_runner_public/orb_calls.rs"]
mod orb_calls;
#[path = "rust_runner_public/removal.rs"]
mod removal;
#[path = "rust_runner_public/runner_copy.rs"]
mod runner_copy;
#[path = "rust_runner_public/store.rs"]
mod store;
#[path = "rust_runner_public/support.rs"]
mod support;
#[path = "rust_runner_public/user.rs"]
mod user;

use std::error::Error;
use std::time::{Duration, Instant};

use velnor_runner_host::{HostConfig, HostError, LaunchMode, RunningJob, RustRunnerFailure};

use support::{
    Fixture, GUEST, SECRET_JIT, jit, process_exists, reservation, runner_lines, wait_for,
};

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_starts_run_jit_per_job_with_distinct_files() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::ready()?;
    let launcher = fixture.launcher();
    let mut first = launcher.start_job(reservation(11)?, &jit("first-jit"))?;
    let mut second = launcher.start_job(reservation(12)?, &jit("second-jit"))?;
    assert!(first.wait().await?.success());
    assert!(second.wait().await?.success());

    let log = fixture.log()?;
    let lines = runner_lines(&log);
    assert_eq!(lines.len(), 2, "{log:?}");
    assert!(
        lines.contains(&&format!(
            "{GUEST} unshare --user --map-current-user --pid --fork --kill-child /var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor-host/jit/11.jit --work-dir /srv/velnor/gen/work/11"
        )),
        "{lines:?}"
    );
    assert!(
        lines.contains(&&format!(
            "{GUEST} unshare --user --map-current-user --pid --fork --kill-child /var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor-host/jit/12.jit --work-dir /srv/velnor/gen/work/12"
        )),
        "{lines:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.seen("11.jit"))?,
        "first-jit"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.seen("12.jit"))?,
        "second-jit"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.seen("11.jit.mode"))?.trim(),
        "600"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_keeps_jit_out_of_argv() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::ready()?;
    let mut job = fixture
        .launcher()
        .start_job(reservation(21)?, &jit(SECRET_JIT))?;
    assert!(job.wait().await?.success());

    assert_eq!(std::fs::read_to_string(fixture.seen("21.jit"))?, SECRET_JIT);
    assert!(
        fixture.log()?.iter().all(|line| !line.contains(SECRET_JIT)),
        "{:?}",
        fixture.log()?
    );
    assert!(!std::fs::read_to_string(fixture.seen("21.jit.env"))?.contains(SECRET_JIT));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_removes_jit_file_after_exit() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::ready()?;
    fixture.configure("FAKE_RUNNER_EXIT=3\n")?;
    let mut job = fixture
        .launcher()
        .start_job(reservation(31)?, &jit(SECRET_JIT))?;
    let status = job.wait().await?;
    assert_eq!(status.code(), Some(3));
    assert!(!fixture.jit_file(31).exists());

    fixture.configure("FAKE_RUNNER_HOLD=1\n")?;
    let job: RunningJob = fixture
        .launcher()
        .start_job(reservation(32)?, &jit(SECRET_JIT))?;
    let child_file = fixture.seen("32.jit.child");
    wait_for(&child_file).await?;
    let child = std::fs::read_to_string(&child_file)?.trim().to_owned();
    assert!(fixture.jit_file(32).exists());
    assert!(process_exists(&child));
    job.stop().await?;
    assert!(!fixture.jit_file(32).exists());
    wait_until_dead(&child).await?;

    let job = fixture
        .launcher()
        .start_job(reservation(33)?, &jit(SECRET_JIT))?;
    let pid_file = fixture.seen("33.jit.pid");
    wait_for(&pid_file).await?;
    wait_for(&fixture.seen("33.jit.child")).await?;
    let pid = std::fs::read_to_string(&pid_file)?.trim().to_owned();
    assert!(fixture.jit_file(33).exists());
    drop(job);
    wait_until_dead(&pid).await?;
    assert!(!fixture.jit_file(33).exists());
    let orphan = std::fs::read_to_string(fixture.seen("33.jit.child"))?
        .trim()
        .to_owned();
    std::process::Command::new("kill").arg(&orphan).status()?;
    wait_until_dead(&orphan).await
}

async fn wait_until_dead(pid: &str) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_exists(pid) {
        if Instant::now() > deadline {
            return Err(format!("process {pid} survived").into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

#[test]
fn rust_runner_fails_when_binary_missing() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::ready()?;
    std::fs::remove_file(fixture.root.join("host-runner"))?;
    let error = fixture
        .launcher()
        .ensure_machine()
        .err()
        .ok_or("ensure_machine accepted a missing binary")?;
    assert_eq!(
        error,
        HostError::RustRunner(RustRunnerFailure::BinaryMissing)
    );
    assert_eq!(
        error.to_string(),
        "velnor-runner is missing from the host runner path"
    );
    Ok(())
}

fn host_config(launch_mode: Option<&str>, platform: &str, name: &str, labels: &[&str]) -> String {
    let labels = labels
        .iter()
        .map(|label| format!("\"{label}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let launch_mode =
        launch_mode.map_or_else(String::new, |mode| format!("launch_mode = \"{mode}\"\n"));
    format!(
        "schema = 2\n\
         [[connections]]\n\
         connection_id = \"lab\"\n\
         repository_id = 42\n\
         repository = \"tailrocks/velnor-lab\"\n\
         credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
         [connections.scale_set]\n\
         name = \"{name}\"\n\
         labels = [{labels}]\n\
         [connections.source_policy]\n\
         mode = \"branch_restricted\"\n\
         branches = [\"main\"]\n\
         tag_prefixes = []\n\
         events = [\"push\"]\n\
         pull_request_rules = []\n\
         [host]\n\
         {launch_mode}\
         max_jobs = 1\n\
         [host.resources]\n\
         runner_cpu_millicores = 2000\n\
         runner_memory_bytes = 4294967296\n\
         dind_cpu_millicores = 1000\n\
         dind_memory_bytes = 2147483648\n\
         operational_memory_reserve_bytes = 1073741824\n\
         [docker]\n\
         context = \"orbstack\"\n\
         platform = \"{platform}\"\n\
         endpoint = \"unix:///var/run/docker.sock\"\n"
    )
}

const MAC_LABELS: [&str; 2] = ["velnor", "velnor-mac"];

#[test]
fn launch_mode_defaults_to_official_runner() -> Result<(), HostError> {
    let parsed = HostConfig::parse(&host_config(
        None,
        "linux/amd64",
        "ubuntu-26.04-scale-set",
        &["velnor", "ubuntu-26.04-scale-set"],
    ))?;
    assert_eq!(parsed.host.launch_mode, LaunchMode::OfficialRunner);
    assert!(
        parsed
            .to_toml()?
            .contains("launch_mode = \"official-runner\"")
    );
    Ok(())
}

#[test]
fn launch_mode_rust_runner_parses() -> Result<(), HostError> {
    let text = host_config(
        Some("rust-runner"),
        "linux/arm64",
        "velnor-mac",
        &MAC_LABELS,
    );
    let parsed = HostConfig::parse_draft(&text)?;
    assert_eq!(parsed.host.launch_mode, LaunchMode::RustRunner);
    assert_eq!(parsed.to_toml().err(), Some(HostError::Config));
    assert_eq!(HostConfig::parse(&text).err(), Some(HostError::Config));
    Ok(())
}

#[test]
fn launch_mode_unknown_value_is_rejected() {
    for mode in ["docker", "rust_runner", "Rust-Runner", ""] {
        assert_eq!(
            HostConfig::parse(&host_config(
                Some(mode),
                "linux/arm64",
                "velnor-mac",
                &MAC_LABELS
            ))
            .err(),
            Some(HostError::Config),
            "{mode}"
        );
    }
}

#[test]
fn rust_runner_mode_accepts_velnor_mac_selector_and_requires_arm64() {
    assert!(
        HostConfig::parse_draft(&host_config(
            Some("rust-runner"),
            "linux/arm64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .is_ok()
    );
    assert_eq!(
        HostConfig::parse_draft(&host_config(
            Some("rust-runner"),
            "linux/amd64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .err(),
        Some(HostError::Config)
    );
    assert_eq!(
        HostConfig::parse(&host_config(
            Some("official-runner"),
            "linux/arm64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .err(),
        Some(HostError::PlatformSelector)
    );
}

#[test]
fn rust_runner_bounds_the_output_it_captures_from_orb() -> Result<(), Box<dyn Error>> {
    removal::orb_output_over_the_capture_bound()
}

#[test]
fn rust_runner_allows_one_controller_per_host_store() -> Result<(), Box<dyn Error>> {
    removal::one_controller_per_host_store()
}

#[test]
fn rust_runner_ignores_a_host_store_override_in_the_environment() -> Result<(), Box<dyn Error>> {
    removal::ignores_a_store_override_in_the_environment(
        "rust_runner_ignores_a_host_store_override_in_the_environment",
    )
}

fn with_machine_disk(lines: &str) -> String {
    host_config(
        Some("rust-runner"),
        "linux/arm64",
        "velnor-mac",
        &MAC_LABELS,
    )
    .replace(
        "[docker]",
        &format!("[host.machine_disk]\n{lines}\n[docker]"),
    )
}

#[test]
fn rust_runner_machine_cap_is_the_store_budget_plus_the_job_allowance_per_job()
-> Result<(), Box<dyn Error>> {
    let default = HostConfig::parse_draft(&host_config(
        Some("rust-runner"),
        "linux/arm64",
        "velnor-mac",
        &MAC_LABELS,
    ))?;
    assert_eq!(default.host.machine_disk_cap_gib(), Some(172));
    let sized = HostConfig::parse_draft(&with_machine_disk(
        "store_budget_gib = 10\njob_limit_gib = 20\njob_slack_gib = 5",
    ))?;
    assert_eq!(sized.host.machine_disk_cap_gib(), Some(35));
    for invalid in [
        "job_limit_gib = 0",
        "store_budget_gib = 18446744073709551615",
        "job_slack_gib = 18446744073709551615",
        "store_budget_gib = 20000",
        "unknown_gib = 1",
    ] {
        assert_eq!(
            HostConfig::parse_draft(&with_machine_disk(invalid)).err(),
            Some(HostError::Config),
            "{invalid}"
        );
    }
    Ok(())
}

#[test]
fn the_machine_disk_survives_a_host_toml_round_trip() -> Result<(), HostError> {
    let official = host_config(
        None,
        "linux/amd64",
        "ubuntu-26.04-scale-set",
        &["velnor", "ubuntu-26.04-scale-set"],
    );
    let plain = HostConfig::parse(&official)?.to_toml()?;
    assert!(!plain.contains("machine_disk"), "{plain}");
    let sized = official.replace(
        "[docker]",
        "[host.machine_disk]\nstore_budget_gib = 10\njob_limit_gib = 20\njob_slack_gib = 5\n[docker]",
    );
    let written = HostConfig::parse(&sized)?.to_toml()?;
    assert_eq!(
        HostConfig::parse(&written)?.host.machine_disk_cap_gib(),
        Some(35)
    );
    Ok(())
}
