// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use super::{RunningSelector, wait_until};
use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_clock::SupervisorClock;
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{expect, expect_flag, produce_flag, selected_unit, unit_key};

#[test]
fn aggregate_expiry_keeps_only_success_observed_before_expiry() -> Result<(), Box<dyn Error>> {
    run_aggregate_status_case("before", 4)?;
    run_aggregate_status_case("equal", 5)?;
    run_aggregate_status_case("late", 6)
}

fn run_aggregate_status_case(label: &str, advance: u64) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new(
        &format!("seal-select-aggregate-status-{label}"),
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let completed = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let delayed = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
    gh.list(&[
        ("2026-11-04T03:00:00Z", MAIN, &completed),
        ("2026-11-04T02:00:00Z", MAIN, &delayed),
    ])?;
    gh.attest(&[
        Attestation::signed(&completed),
        Attestation::signed(&delayed),
    ])?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    let real_python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;
    let gate = PathBuf::from(gh.dir()).join("verify-release");
    let started = PathBuf::from(gh.dir()).join("verify-started");
    let gate_value = gate.to_string_lossy().into_owned();
    let started_value = started.to_string_lossy().into_owned();
    let group_log = PathBuf::from(gh.dir())
        .join("group-log")
        .to_string_lossy()
        .into_owned();
    let mut selector = RunningSelector::new(
        crate::gc1_seal_units::spawn_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, "."), (StateUnit::Union, "b")],
            &[
                ("VELNOR_STATE_GH_TIMEOUT", "30"),
                ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
                ("VELNOR_STATE_SELECT_DEADLINE", "5"),
                ("FAKE_REAL_PYTHON3", real_python),
                ("FAKE_GH_VERIFY_GATE", &gate_value),
                ("FAKE_GH_VERIFY_GATE_KEY", &delayed),
                ("FAKE_GH_VERIFY_STARTED", &started_value),
                ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
                ("FAKE_SUPERVISOR_GROUP_LOG", &group_log),
            ],
        )?,
        &gh,
    );
    wait_until(|| started.exists(), Duration::from_secs(10))?;
    let state_dir = scene.runner.join("velnor/state-select");
    hold_first_status_until_processed(&clock, &completed, &state_dir)?;
    let second_barrier = format!("second-{label}");
    clock.block(&second_barrier)?;
    clock.wait_for_block(&second_barrier, Duration::from_secs(10))?;
    fs::write(&gate, "release\n")?;
    let second_status = state_dir.join("status-u1");
    wait_until(|| second_status.exists(), Duration::from_secs(10))?;
    clock.advance(&second_barrier, advance)?;
    clock.release_block(&second_barrier)?;
    clock.wait_for_advance(&second_barrier, Duration::from_secs(10))?;
    wait_for_success(&mut selector)?;
    super::aggregate_cleanup::assert_groups_reaped(&group_log)?;
    assert_aggregate_outcome(&scene, &completed, &delayed, advance)?;
    scene.finish()
}

fn wait_for_success(selector: &mut RunningSelector) -> Result<(), Box<dyn Error>> {
    if !selector.wait(Duration::from_secs(10))?.success() {
        return Err(fail("aggregate expiry failed the selector step"));
    }
    Ok(())
}

fn assert_aggregate_outcome(
    scene: &Scene,
    completed: &str,
    delayed: &str,
    advance: u64,
) -> Result<(), Box<dyn Error>> {
    expect(
        &selected_unit(scene, StateUnit::Union, ".")?,
        &[completed.to_owned()],
        "success observed before aggregate expiry",
    )?;
    expect_flag(
        &produce_flag(scene, StateUnit::Union, ".")?,
        "false",
        "success observed before aggregate expiry",
    )?;
    let (second_keys, second_produce) = if advance < 5 {
        (vec![delayed.to_owned()], "false")
    } else {
        (Vec::new(), "true")
    };
    expect(
        &selected_unit(scene, StateUnit::Union, "b")?,
        &second_keys,
        "success status observed around aggregate expiry",
    )?;
    expect_flag(
        &produce_flag(scene, StateUnit::Union, "b")?,
        second_produce,
        "success status observed around aggregate expiry",
    )?;
    Ok(())
}

#[test]
fn aggregate_expiry_after_status_read_rejects_late_acceptance() -> Result<(), Box<dyn Error>> {
    let mut case = aggregate_status_read_case()?;
    let state_dir = case.scene.runner.join("velnor/state-select");
    let started = case.gh_dir.join("verify-started");
    wait_until(|| started.exists(), Duration::from_secs(10))?;
    hold_first_status_until_processed(&case.clock, &case.completed, &state_dir)?;
    let read_barrier = "status-read";
    case.clock.block(read_barrier)?;
    case.clock
        .wait_for_block(read_barrier, Duration::from_secs(10))?;
    fs::write(&case.gate, "release\n")?;
    let status_path = state_dir.join("status-u1");
    wait_until(|| status_path.exists(), Duration::from_secs(10))?;
    let barrier = start_status_fifo_writer(status_path, &case.gh_dir)?;
    case.clock.release_block(read_barrier)?;
    wait_until(|| barrier.opened.exists(), Duration::from_secs(10))?;
    case.clock.advance(read_barrier, 6)?;
    fs::write(&barrier.release, "release\n")?;
    case.clock
        .wait_for_advance(read_barrier, Duration::from_secs(10))?;
    barrier
        .writer
        .join()
        .map_err(|_| fail("status-read barrier writer panicked"))??;
    let status = case.selector.wait(Duration::from_secs(10))?;
    if !status.success() {
        return Err(fail("aggregate expiry failed the selector step"));
    }
    super::aggregate_cleanup::assert_groups_reaped(&case.group_log)?;
    expect(
        &selected_unit(&case.scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&case.completed),
        "success observed before aggregate expiry",
    )?;
    expect(
        &selected_unit(&case.scene, StateUnit::Union, "b")?,
        &[],
        "status read before aggregate expiry and accepted after",
    )?;
    expect_flag(
        &produce_flag(&case.scene, StateUnit::Union, "b")?,
        "true",
        "status read before aggregate expiry and accepted after",
    )?;
    case.scene.finish()
}

struct AggregateStatusReadCase {
    scene: Scene,
    clock: SupervisorClock,
    selector: RunningSelector,
    completed: String,
    gate: PathBuf,
    gh_dir: PathBuf,
    group_log: String,
}

struct StatusFifoBarrier {
    writer: thread::JoinHandle<std::io::Result<()>>,
    opened: PathBuf,
    release: PathBuf,
}

fn aggregate_status_read_case() -> Result<AggregateStatusReadCase, Box<dyn Error>> {
    let scene = Scene::new(
        "seal-select-aggregate-status-read",
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let completed = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let delayed = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
    gh.list(&[
        ("2026-11-04T03:00:00Z", MAIN, &completed),
        ("2026-11-04T02:00:00Z", MAIN, &delayed),
    ])?;
    gh.attest(&[
        Attestation::signed(&completed),
        Attestation::signed(&delayed),
    ])?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    let real_python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;
    let gate = PathBuf::from(gh.dir()).join("verify-release");
    let started = PathBuf::from(gh.dir()).join("verify-started");
    let gh_dir = PathBuf::from(gh.dir());
    let gate_value = gate.to_string_lossy().into_owned();
    let started_value = started.to_string_lossy().into_owned();
    let group_log = PathBuf::from(gh.dir())
        .join("group-log")
        .to_string_lossy()
        .into_owned();
    let selector = RunningSelector::new(
        crate::gc1_seal_units::spawn_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, "."), (StateUnit::Union, "b")],
            &[
                ("VELNOR_STATE_GH_TIMEOUT", "30"),
                ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
                ("VELNOR_STATE_SELECT_DEADLINE", "5"),
                ("FAKE_REAL_PYTHON3", real_python),
                ("FAKE_GH_VERIFY_GATE", &gate_value),
                ("FAKE_GH_VERIFY_GATE_KEY", &delayed),
                ("FAKE_GH_VERIFY_STARTED", &started_value),
                ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
                ("FAKE_SUPERVISOR_GROUP_LOG", &group_log),
            ],
        )?,
        &gh,
    );
    Ok(AggregateStatusReadCase {
        scene,
        clock,
        selector,
        completed,
        gate,
        gh_dir,
        group_log,
    })
}

fn hold_first_status_until_processed(
    clock: &SupervisorClock,
    completed: &str,
    state_dir: &std::path::Path,
) -> Result<(), Box<dyn Error>> {
    let first_output = state_dir.join("u-r0/out");
    let first_status = state_dir.join("status-u0");
    let barrier = "first-read-boundary";
    clock.block(barrier)?;
    clock.wait_for_block(barrier, Duration::from_secs(10))?;
    let first_status_ready = wait_until(|| first_status.exists(), Duration::from_secs(1)).is_ok();
    if !first_status_ready
        && !fs::read_to_string(&first_output)?
            .lines()
            .any(|line| line == completed)
    {
        return Err(fail(
            "first successful unit was neither pending nor complete",
        ));
    }
    clock.release_block(barrier)?;
    if first_status_ready {
        wait_until(|| !first_status.exists(), Duration::from_secs(10))?;
    }
    Ok(())
}

fn start_status_fifo_writer(
    status_path: PathBuf,
    gh_dir: &std::path::Path,
) -> Result<StatusFifoBarrier, Box<dyn Error>> {
    fs::remove_file(&status_path)?;
    if !Command::new("mkfifo").arg(&status_path).status()?.success() {
        return Err(fail("could not create the status-read barrier"));
    }
    let opened = gh_dir.join("status-fifo-opened");
    let release = gh_dir.join("status-fifo-release");
    let opened_for_writer = opened.clone();
    let release_for_writer = release.clone();
    let writer = thread::spawn(move || -> std::io::Result<()> {
        let mut status = OpenOptions::new().write(true).open(status_path)?;
        fs::write(opened_for_writer, "open\n")?;
        while !release_for_writer.exists() {
            thread::sleep(Duration::from_millis(10));
        }
        status.write_all(b"0\n")?;
        Ok(())
    });
    Ok(StatusFifoBarrier {
        writer,
        opened,
        release,
    })
}
