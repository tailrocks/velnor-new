// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rustix::process::{Pid, Signal, kill_process};
use tokio::process::{Child, Command};

use super::exec_builder::{BACKGROUND_SLEEP, assert_gone, pid_from};
use super::exec_support::{Scratch, fixture_path};
use super::lifecycle_support::{fixture_job, job_replacing};
use super::support::mock_github::{Behavior, MockGithub, Online, SESSION_ID};

const EXIT_BUDGET: Duration = Duration::from_secs(20);

async fn velnor_runner(arguments: &[&str]) -> Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_velnor-runner"))
        .args(arguments)
        .output()
        .await
        .context("run velnor-runner")
}

pub(crate) struct Started {
    pub(crate) child: Child,
    pub(crate) jit_file: PathBuf,
    pub(crate) work_dir: PathBuf,
}

pub(crate) fn start_runner(mock: &MockGithub, scratch: &Scratch) -> Result<Started> {
    std::fs::create_dir_all(scratch.work_dir())?;
    let jit_file = scratch.work_dir().join("jit.txt");
    std::fs::write(&jit_file, mock.jit())?;
    let work_dir = scratch.work_dir().join("job");
    let child = Command::new(env!("CARGO_BIN_EXE_velnor-runner"))
        .arg("run")
        .arg("--jit")
        .arg(format!("@{}", jit_file.display()))
        .arg("--work-dir")
        .arg(&work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("start velnor-runner")?;
    Ok(Started {
        child,
        jit_file,
        work_dir,
    })
}

pub(crate) async fn until<T>(
    what: &str,
    mut probe: impl FnMut() -> Result<Option<T>>,
) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(found) = probe()? {
            return Ok(found);
        }
        if Instant::now() > deadline {
            bail!("{what} never happened");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub(crate) async fn signal_and_wait(started: Started, signal: Signal) -> Result<Output> {
    let runner = started.child.id().context("the runner pid")?;
    let runner = Pid::from_raw(i32::try_from(runner)?).context("a valid runner pid")?;
    kill_process(runner, signal).context("send the signal")?;
    tokio::time::timeout(EXIT_BUDGET, started.child.wait_with_output())
        .await
        .context("the runner exited after the signal")?
        .context("wait for the runner")
}

fn pid_file(work_dir: &Path) -> PathBuf {
    work_dir.join("temp").join("bg.pid")
}

#[tokio::test]
async fn run_cli_rejects_invalid_jit_config() -> Result<()> {
    let output = velnor_runner(&["run", "--jit", "not-base64"]).await?;

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("decode encoded_jit_config"), "{stderr}");
    Ok(())
}

#[tokio::test]
async fn run_cli_requires_jit_argument() -> Result<()> {
    let output = velnor_runner(&["run"]).await?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--jit"));
    Ok(())
}

#[tokio::test]
async fn run_cli_runs_the_job_of_a_jit_file() -> Result<()> {
    let job = serde_json::from_str(&std::fs::read_to_string(fixture_path("hello"))?)?;
    let mock = MockGithub::start(job, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    std::fs::create_dir_all(scratch.work_dir())?;
    let jit_file = scratch.work_dir().join("jit.txt");
    std::fs::write(&jit_file, mock.jit())?;
    let jit_argument = format!("@{}", jit_file.display());
    let work_dir = scratch.work_dir().join("job");

    let output = velnor_runner(&[
        "run",
        "--jit",
        &jit_argument,
        "--work-dir",
        &work_dir.display().to_string(),
    ])
    .await?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    assert!(!jit_file.exists(), "the JIT file holds the RSA key");
    Ok(())
}

#[tokio::test]
async fn run_cli_deletes_the_jit_file_once_it_is_read() -> Result<()> {
    let scratch = Scratch::new()?;
    std::fs::create_dir_all(scratch.work_dir())?;
    let jit_file = scratch.work_dir().join("jit.txt");
    std::fs::write(&jit_file, "not-base64")?;

    let output = velnor_runner(&["run", "--jit", &format!("@{}", jit_file.display())]).await?;

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("decode encoded_jit_config"),
        "the file was read before it was deleted"
    );
    assert!(!jit_file.exists(), "the JIT file holds the RSA key");
    Ok(())
}

#[tokio::test]
async fn run_cli_refuses_to_start_when_the_jit_file_cannot_be_deleted() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if rustix::process::geteuid().is_root() {
        bail!("the directory permission below does not bind root; run the test as a normal user");
    }
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let directory = scratch.work_dir().join("sealed");
    std::fs::create_dir_all(&directory)?;
    let jit_file = directory.join("jit.txt");
    std::fs::write(&jit_file, mock.jit())?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555))?;

    let output = velnor_runner(&["run", "--jit", &format!("@{}", jit_file.display())]).await;

    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))?;
    let output = output?;
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("remove the JIT file"), "{stderr}");
    assert!(mock.server.requests()?.is_empty(), "the runner went on");
    Ok(())
}

#[tokio::test]
async fn run_cli_exits_1_and_deletes_the_session_when_the_job_cannot_be_acquired() -> Result<()> {
    let behavior = Behavior {
        unusable_job: true,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;
    let started = start_runner(&mock, &scratch)?;

    let output = tokio::time::timeout(EXIT_BUDGET, started.child.wait_with_output())
        .await
        .context("the runner exited")??;

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("acquire the job"), "{stderr}");
    let delete = mock.server.only_request("DELETE", "/broker/session")?;
    assert_eq!(delete.query("sessionId"), Some(SESSION_ID));
    Ok(())
}

async fn signal_cancels_the_running_job(signal: Signal) -> Result<()> {
    let job = job_replacing("cancel", Some(("sleep 60", BACKGROUND_SLEEP)))?;
    let mock = MockGithub::start(job, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let started = start_runner(&mock, &scratch)?;
    let pid_file = pid_file(&started.work_dir);
    let step_process = until("the step started", || {
        Ok(pid_file
            .exists()
            .then(|| pid_from(&pid_file).ok())
            .flatten())
    })
    .await?;
    assert!(!started.jit_file.exists());

    let output = signal_and_wait(started, signal).await?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "canceled");
    let conclusions: Vec<_> = complete["stepResults"]
        .as_array()
        .context("step results")?
        .iter()
        .map(|step| step["conclusion"].clone())
        .collect();
    assert_eq!(conclusions, ["canceled", "succeeded"]);
    let delete = mock.server.only_request("DELETE", "/broker/session")?;
    assert_eq!(delete.query("sessionId"), Some(SESSION_ID));
    assert_gone(step_process)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_sigterm_cancels_the_running_job() -> Result<()> {
    signal_cancels_the_running_job(Signal::TERM).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_sigint_cancels_the_running_job() -> Result<()> {
    signal_cancels_the_running_job(Signal::INT).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_sigterm_while_waiting_for_a_job_deletes_the_session() -> Result<()> {
    let behavior = Behavior {
        online: Online::Idle,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;
    let started = start_runner(&mock, &scratch)?;
    until("the first poll", || {
        Ok((mock.polls("Online")? > 0).then_some(()))
    })
    .await?;

    let output = signal_and_wait(started, Signal::TERM).await?;

    assert_eq!(output.status.code(), Some(0));
    let delete = mock.server.only_request("DELETE", "/broker/session")?;
    assert_eq!(delete.query("sessionId"), Some(SESSION_ID));
    assert!(mock.requests_to("POST", "/run/acquirejob")?.is_empty());
    Ok(())
}

const LOG_LINES: usize = 100_000;
const LOG_LINE_BYTES: usize = 2_000;
const PEAK_RSS_CEILING_KIB: u64 = 40 * 1024;

#[cfg(target_os = "linux")]
fn high_water_kib(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .trim()
        .strip_suffix("kB")?
        .trim()
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
async fn peak_resident_kib(pid: u32) -> Option<u64> {
    let status = tokio::fs::read_to_string(format!("/proc/{pid}/status"))
        .await
        .ok()?;
    high_water_kib(&status)
}

#[cfg(not(target_os = "linux"))]
async fn peak_resident_kib(pid: u32) -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

async fn run_to_exit_sampling_memory(started: Started) -> Result<(Output, u64)> {
    let Started { mut child, .. } = started;
    let pid = child.id().context("the runner pid")?;
    let mut peak = 0;
    let mut samples = 0_u32;
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait()?.is_none() {
        anyhow::ensure!(Instant::now() < deadline, "the runner did not exit");
        if let Some(kib) = peak_resident_kib(pid).await {
            samples += 1;
            peak = peak.max(kib);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::ensure!(
        samples > 0 && peak > 0,
        "no memory sample of the runner was read ({samples} samples, peak {peak} KiB)"
    );
    Ok((child.wait_with_output().await?, peak))
}

#[tokio::test]
async fn run_cli_peak_memory_stays_below_the_ceiling_while_the_log_is_far_larger() -> Result<()> {
    let script = format!("yes {} | head -n {LOG_LINES}", "x".repeat(LOG_LINE_BYTES));
    let mock = MockGithub::start(fixture_job("hello", Some(&script))?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let started = start_runner(&mock, &scratch)?;

    let (output, peak_kib) = run_to_exit_sampling_memory(started).await?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let uploaded: usize = mock
        .requests_to("PUT", super::support::mock_github::JOB_LOG_BLOB)?
        .iter()
        .map(|put| put.text().len())
        .sum();
    assert!(
        uploaded >= LOG_LINES * LOG_LINE_BYTES,
        "the whole job log reached GitHub: {uploaded} bytes"
    );
    assert!(
        peak_kib < PEAK_RSS_CEILING_KIB,
        "peak resident memory {peak_kib} KiB is over {PEAK_RSS_CEILING_KIB} KiB while the log is {uploaded} bytes"
    );
    Ok(())
}

#[test]
fn run_args_debug_output_does_not_show_the_jit_value() -> Result<()> {
    use clap::Parser;

    let cli = velnor_runner::cli::Cli::try_parse_from([
        "velnor-runner",
        "run",
        "--jit",
        "encoded-jit-secret-value",
    ])?;

    let shown = format!("{cli:?}");

    assert!(!shown.contains("encoded-jit-secret-value"), "{shown}");
    Ok(())
}
