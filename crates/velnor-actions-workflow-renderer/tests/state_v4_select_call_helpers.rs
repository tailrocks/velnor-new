use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use super::RunningSelector;
use crate::gc1_scene::fail;
use crate::gc1_seal_clock::{ClockBlock, SupervisorClock};
use crate::gc1_seal_support::Gh;
use crate::gc1_seal_units::{group_is_live, process_group, process_is_running};
use velnor_actions_contract::workflow::cache_family::StateUnit;

#[path = "state_v4_select_fifo_custody.rs"]
mod fifo_custody;

pub(super) use fifo_custody::{
    spawn_fifo_reader, start_status_fifo_writer, terminate_group_bounded,
};

pub(super) fn wait_for_block_or_completion(
    selector: &mut RunningSelector,
    gh_dir: &Path,
    label: &str,
    timeout: Duration,
) -> Result<Option<ExitStatus>, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = selector.observe(Duration::from_millis(10))? {
            return Ok(Some(status));
        }
        if gh_dir
            .join(format!("clock-block-ack-control-{label}"))
            .exists()
        {
            return Ok(None);
        }
        if Instant::now() >= deadline {
            return Err(fail("configured budget clock block watchdog expired"));
        }
    }
}

pub(super) fn clear_readiness_path(path: &Path) -> Result<(), Box<dyn Error>> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

pub(super) fn read_published_pid(path: &Path) -> Result<u32, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let mut lines = content.lines();
    let pid: u32 = lines
        .next()
        .ok_or_else(|| fail("listing PID publication was empty"))?
        .parse()?;
    if pid == 0 || lines.next().is_some() {
        return Err(fail("listing PID publication was not one positive PID"));
    }
    Ok(pid)
}

pub(super) fn wait_for_live_listing_pid(
    readiness_path: &Path,
    legacy_path: &Path,
    timeout: Duration,
) -> Result<(u32, u32), Box<dyn Error>> {
    super::wait_until(|| readiness_path.exists(), timeout)?;
    let pid = read_published_pid(readiness_path)?;
    if pid != read_published_pid(legacy_path)? || !process_is_running(pid) {
        return Err(fail(
            "listing PID did not identify the live fake GH invocation",
        ));
    }
    let group = process_group(pid).ok_or_else(|| fail("missing listing process group"))?;
    if group == 0 {
        return Err(fail("invalid listing process group"));
    }
    Ok((pid, group))
}

pub(super) fn assert_listing_group_reaped(
    pid: u32,
    group: u32,
    message: &str,
) -> Result<(), Box<dyn Error>> {
    if process_is_running(pid)
        || PathBuf::from(format!("/proc/{group}")).exists()
        || group_is_live(group)
    {
        return Err(fail(message));
    }
    Ok(())
}

pub(super) fn await_budget_boundary(
    clock: &SupervisorClock,
    selector: &mut RunningSelector,
    gh_dir: &Path,
    label: &str,
    before_expiry: u64,
) -> Result<(Option<ExitStatus>, Vec<ClockBlock>), Box<dyn Error>> {
    let mut releases = Vec::new();
    releases.push(clock.block_guarded("budget-one")?);
    clock.wait_for_block("budget-one", Duration::from_secs(10))?;
    releases.push(clock.block_guarded("budget-two")?);
    clock.advance(label, before_expiry)?;
    releases[0].release()?;
    clock.wait_for_advance(label, Duration::from_secs(10))?;
    let mut early_status =
        wait_for_block_or_completion(selector, gh_dir, "budget-two", Duration::from_secs(10))?;
    if early_status.is_none() {
        releases.push(clock.block_guarded("budget-three")?);
        releases[1].release()?;
        early_status = wait_for_block_or_completion(
            selector,
            gh_dir,
            "budget-three",
            Duration::from_secs(10),
        )?;
    }
    releases[1].release()?;
    if releases.len() == 3 {
        releases[2].release()?;
    }
    Ok((early_status, releases))
}

pub(super) fn spawn_configured_listing_selector(
    scene: &crate::gc1_scene::Scene,
    gh: &Gh,
    clock: &SupervisorClock,
    label: &str,
    call_limit: Option<&str>,
) -> Result<(RunningSelector, PathBuf, PathBuf), Box<dyn Error>> {
    let real_python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;
    let gh_dir = PathBuf::from(gh.dir());
    let ready_path = gh_dir.join(format!("list-ready-{label}"));
    clear_readiness_path(&ready_path)?;
    let ready_path_value = ready_path
        .to_str()
        .ok_or_else(|| fail("invalid listing readiness path"))?;
    let mut extra = vec![
        ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
        ("VELNOR_STATE_SELECT_DEADLINE", "600"),
        ("FAKE_REAL_PYTHON3", real_python),
        ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
        ("FAKE_GH_LIST_PID_READY", ready_path_value),
    ];
    if let Some(limit) = call_limit {
        extra.push(("VELNOR_STATE_GH_TIMEOUT", limit));
    }
    let selector = RunningSelector::new(
        crate::gc1_seal_units::spawn_select_units(scene, gh, &[(StateUnit::Union, ".")], &extra)?,
        gh,
    );
    Ok((selector, gh_dir, ready_path))
}
