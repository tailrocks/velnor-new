// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::exec_support::Scratch;
use super::lifecycle_support::fixture_job;
use super::run_cli::{Started, start_runner_with};
use super::support::mock_github::{Behavior, JOB_LOG_BLOB, MockGithub};

const BASELINE_LINES: usize = 12_500;
const GROWTH: usize = 8;
const ARENA_LIMIT: &str = "MALLOC_ARENA_MAX";
const LOG_LINE_BYTES: usize = 2_000;
const PEAK_GROWTH_ALLOWED_KIB: u64 = 8 * 1024;
const EXIT_SAFETY_NET: Duration = Duration::from_secs(600);

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
    let output = tokio::process::Command::new("ps")
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
    let deadline = Instant::now() + EXIT_SAFETY_NET;
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

async fn peak_kib_for_log_of(lines: usize) -> Result<u64> {
    let script = format!("yes {} | head -n {lines}", "x".repeat(LOG_LINE_BYTES));
    let mock = MockGithub::start(fixture_job("hello", Some(&script))?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let started = start_runner_with(&mock, &scratch, None, &[(ARENA_LIMIT, Path::new("1"))])?;

    let (output, peak_kib) = run_to_exit_sampling_memory(started).await?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let uploaded: usize = mock
        .requests_to("PUT", JOB_LOG_BLOB)?
        .iter()
        .map(|put| put.text().len())
        .sum();
    assert!(
        uploaded >= lines * LOG_LINE_BYTES,
        "the whole job log reached GitHub: {uploaded} bytes"
    );
    Ok(peak_kib)
}

#[tokio::test]
async fn run_cli_peak_memory_stays_flat_while_the_log_grows() -> Result<()> {
    let baseline = peak_kib_for_log_of(BASELINE_LINES).await?;

    let grown = peak_kib_for_log_of(BASELINE_LINES * GROWTH).await?;

    assert!(
        grown <= baseline + PEAK_GROWTH_ALLOWED_KIB,
        "peak resident memory grew from {baseline} KiB to {grown} KiB while the log grew {GROWTH} times"
    );
    Ok(())
}
