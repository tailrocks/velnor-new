// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::time::{Duration, Instant};

use velnor_runner_host::{HostError, RunningJob, RustRunnerFailure};

use super::support::{
    Fixture, SECRET_JIT, facts, jit, process_exists, reservation, runner_lines, wait_for,
};

const HOME_ROOT: &str = "/home";

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_starts_run_jit_per_job_with_distinct_files() -> Result<(), Box<dyn Error>> {
    let fixture =
        Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    let launcher = fixture.launcher();
    let user = launcher.ensure_machine()?;
    let mut first = launcher.start_job(reservation(11)?, &jit("first-jit"), &facts(), &user)?;
    let mut second = launcher.start_job(reservation(12)?, &jit("second-jit"), &facts(), &user)?;
    assert!(first.wait().await?.success());
    assert!(second.wait().await?.success());

    let log = fixture.log()?;
    let lines = runner_lines(&log);
    assert_eq!(lines.len(), 2, "{log:?}");
    assert!(
        lines.contains(
            &&"-m velnor -u root /var/lib/velnor/bin/velnor-runner sandbox --profile job --reservation 11 --uid 501 --gid 501 --home /home/runner --work-dir /srv/velnor/gen/work/11 --disk-limit-gib 64 --max-jobs 1 --stdin-frames -- /var/lib/velnor/bin/velnor-runner run --jit - --work-dir /work"
                .to_owned()
        ),
        "{lines:?}"
    );
    assert!(
        lines.contains(
            &&"-m velnor -u root /var/lib/velnor/bin/velnor-runner sandbox --profile job --reservation 12 --uid 501 --gid 501 --home /home/runner --work-dir /srv/velnor/gen/work/12 --disk-limit-gib 64 --max-jobs 1 --stdin-frames -- /var/lib/velnor/bin/velnor-runner run --jit - --work-dir /work"
                .to_owned()
        ),
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
    for name in ["jit", "trusted"] {
        assert!(
            !fixture.store.join(name).exists(),
            "{name} exists on the host share"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_runs_each_job_as_the_user_ensure_machine_reported()
-> Result<(), Box<dyn Error>> {
    let fixture =
        Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    let launcher = fixture.launcher();
    for (reservation_id, name, uid, gid) in [(14, "builder", 700, 701), (15, "other", 800, 801)] {
        let home = format!("{HOME_ROOT}/{name}");
        let user = format!("{uid} {gid} {home}");
        let expected = format!("--uid {uid} --gid {gid} --home {home}");
        fixture.configure(&format!("FAKE_ORB_USER={user}\n"))?;
        let reported = launcher.ensure_machine()?;
        let mut job = launcher.start_job(
            reservation(reservation_id)?,
            &jit("jit"),
            &facts(),
            &reported,
        )?;
        assert!(job.wait().await?.success());
        let log = fixture.log()?;
        assert!(
            runner_lines(&log)
                .iter()
                .any(|line| line.contains(&expected)),
            "{expected}: {log:?}"
        );
    }
    fixture.configure("FAKE_ORB_USER=501 501 /home/runner extra\n")?;
    assert_eq!(
        launcher.ensure_machine().err(),
        Some(HostError::RustRunner(RustRunnerFailure::GuestUser))
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_writes_the_trusted_facts_the_runner_parses() -> Result<(), Box<dyn Error>> {
    let fixture =
        Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    let launcher = fixture.launcher();
    let user = launcher.ensure_machine()?;
    let mut job = launcher.start_job(reservation(13)?, &jit("jit"), &facts(), &user)?;
    assert!(job.wait().await?.success());
    assert_eq!(
        std::fs::read(fixture.seen("13.json"))?,
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../velnor-runner/tests/fixtures/trusted/pull_request.json"
        ))?
    );
    assert!(
        !fixture.guest_job(13).exists(),
        "the run directory outlived the job"
    );
    assert!(
        !fixture.store.join("trusted").exists() && !fixture.store.join("13.json").exists(),
        "the trusted facts touched the host share"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_keeps_jit_out_of_argv() -> Result<(), Box<dyn Error>> {
    let fixture =
        Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    let launcher = fixture.launcher();
    let user = launcher.ensure_machine()?;
    let mut job = launcher.start_job(reservation(21)?, &jit(SECRET_JIT), &facts(), &user)?;
    assert!(job.wait().await?.success());

    assert_eq!(std::fs::read_to_string(fixture.seen("21.jit"))?, SECRET_JIT);
    assert!(
        fixture.log()?.iter().all(|line| !line.contains(SECRET_JIT)),
        "{:?}",
        fixture.log()?
    );
    assert!(!std::fs::read_to_string(fixture.seen("21.jit.env"))?.contains(SECRET_JIT));
    assert!(!std::fs::read_to_string(fixture.seen("21.jit.argv"))?.contains(SECRET_JIT));
    assert!(
        !tree_contains(&fixture.store, SECRET_JIT.as_bytes())?,
        "the JIT was written under the host share"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_stop_and_drop_end_the_job_processes() -> Result<(), Box<dyn Error>> {
    let fixture =
        Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    let launcher = fixture.launcher();
    let user = launcher.ensure_machine()?;
    fixture.configure("FAKE_RUNNER_EXIT=3\n")?;
    let mut job = launcher.start_job(reservation(31)?, &jit(SECRET_JIT), &facts(), &user)?;
    let status = job.wait().await?;
    assert_eq!(status.code(), Some(3));

    fixture.configure("FAKE_RUNNER_HOLD=1\n")?;
    let job: RunningJob =
        launcher.start_job(reservation(32)?, &jit(SECRET_JIT), &facts(), &user)?;
    let child_file = fixture.seen("32.jit.child");
    wait_for(&child_file).await?;
    let child = std::fs::read_to_string(&child_file)?.trim().to_owned();
    assert!(process_exists(&child));
    job.stop().await?;
    wait_until_dead(&child).await?;

    let job = launcher.start_job(reservation(33)?, &jit(SECRET_JIT), &facts(), &user)?;
    let pid_file = fixture.seen("33.jit.pid");
    wait_for(&pid_file).await?;
    wait_for(&fixture.seen("33.jit.child")).await?;
    let pid = std::fs::read_to_string(&pid_file)?.trim().to_owned();
    drop(job);
    wait_until_dead(&pid).await?;
    let orphan = std::fs::read_to_string(fixture.seen("33.jit.child"))?
        .trim()
        .to_owned();
    std::process::Command::new("kill").arg(&orphan).status()?;
    wait_until_dead(&orphan).await
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_refuses_a_jit_or_facts_over_the_frame_bound_before_spawning()
-> Result<(), Box<dyn Error>> {
    let fixture =
        Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    let launcher = fixture.launcher();
    let user = launcher.ensure_machine()?;
    let before = runner_lines(&fixture.log()?).len();
    let largest = "x".repeat(64 * 1024);
    let mut job = launcher.start_job(reservation(61)?, &jit(&largest), &facts(), &user)?;
    assert!(job.wait().await?.success());
    assert_eq!(std::fs::read_to_string(fixture.seen("61.jit"))?, largest);
    let over = "x".repeat(64 * 1024 + 1);
    assert_eq!(
        launcher
            .start_job(reservation(62)?, &jit(&over), &facts(), &user)
            .err(),
        Some(HostError::RustRunner(RustRunnerFailure::FrameDelivery))
    );
    assert_eq!(runner_lines(&fixture.log()?).len(), before + 1);
    Ok(())
}

#[test]
fn rust_runner_reports_a_cleanup_that_cannot_empty_a_job_dir_as_wedged()
-> Result<(), Box<dyn Error>> {
    let fixture =
        Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    fixture.launcher().ensure_machine()?;
    fixture.configure("FAKE_CLEANUP_WEDGE=1\n")?;
    assert_eq!(
        fixture.launcher().sweep().err(),
        Some(HostError::RustRunner(RustRunnerFailure::JobDirsWedged))
    );
    fixture.configure("")?;
    fixture.launcher().sweep()?;
    Ok(())
}

fn tree_contains(root: &std::path::Path, needle: &[u8]) -> Result<bool, Box<dyn Error>> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        let found = if metadata.is_dir() {
            tree_contains(&path, needle)?
        } else if metadata.is_file() {
            std::fs::read(&path)?
                .windows(needle.len())
                .any(|window| window == needle)
        } else {
            false
        };
        if found {
            return Ok(true);
        }
    }
    Ok(false)
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
