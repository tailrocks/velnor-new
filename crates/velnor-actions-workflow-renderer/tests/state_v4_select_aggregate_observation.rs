// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use super::call_helpers::{spawn_fifo_reader, start_status_fifo_writer};
use super::{RunningSelector, wait_until};
use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_clock::{ReleaseMarker, SupervisorClock};
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{expect, expect_flag, produce_flag, selected_unit, unit_key};

#[test]
fn aggregate_expiry_keeps_only_success_observed_before_expiry() -> Result<(), Box<dyn Error>> {
    run_aggregate_status_case("before", 4)?;
    run_aggregate_status_case("equal", 5)?;
    run_aggregate_status_case("late", 6)
}

fn run_aggregate_status_case(label: &str, advance: u64) -> Result<(), Box<dyn Error>> {
    eprintln!("X101 case aggregate={label}");
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
    let verify_release = ReleaseMarker::new(gate.clone());
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
    let mut verify_release = verify_release;
    wait_until(|| started.exists(), Duration::from_secs(10))?;
    let state_dir = scene.runner.join("velnor/state-select");
    hold_first_status_until_processed(&clock, &completed, &state_dir)?;
    let second_barrier = format!("second-{label}");
    let mut second_release = clock.block_guarded(&second_barrier)?;
    clock.wait_for_block(&second_barrier, Duration::from_secs(10))?;
    verify_release.release()?;
    let second_status = state_dir.join("status-u1");
    wait_until(|| second_status.exists(), Duration::from_secs(10))?;
    clock.advance(&second_barrier, advance)?;
    second_release.release()?;
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
    let mut read_release = case.clock.block_guarded(read_barrier)?;
    case.clock
        .wait_for_block(read_barrier, Duration::from_secs(10))?;
    case.verify_release.release()?;
    let status_path = state_dir.join("status-u1");
    wait_until(|| status_path.exists(), Duration::from_secs(10))?;
    let mut barrier = start_status_fifo_writer(&status_path, &case.gh_dir)?;
    barrier.allow_open()?;
    read_release.release()?;
    wait_until(|| barrier.opened().exists(), Duration::from_secs(10))?;
    case.clock.advance(read_barrier, 6)?;
    barrier.release_and_wait()?;
    case.clock
        .wait_for_advance(read_barrier, Duration::from_secs(10))?;
    let status = case.selector.wait(Duration::from_secs(10))?;
    if !status.success() {
        return Err(fail("aggregate expiry failed the selector step"));
    }
    case.verify_release.release()?;
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

#[test]
#[cfg(target_os = "linux")]
fn status_fifo_custody_covers_preopen_cancel_postopen_cancel_and_delivery()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("status-fifo-custody", crate::gc1_t53::scripts()?)?;
    let directory = scene.scratch.join("fifo-custody");
    fs::create_dir_all(&directory)?;

    let pre = start_status_fifo_writer(&directory.join("pre-status"), &directory)?;
    let mut pre_cleanup = pre.external_cleanup();
    super::wait_until(|| pre.started().exists(), Duration::from_secs(5))?;
    pre.allow_open()?;
    let pre_blocked = observe_fifo_open_without_reader(pre.process_group(), pre.opened());
    let pre_group = pre.process_group();
    let pre_marker = pre.opened().to_owned();
    drop(pre);
    let pre_survived = crate::gc1_seal_units::group_is_live(pre_group);
    pre_cleanup.terminate_and_reap()?;
    if pre_marker.exists() || pre_survived || !pre_blocked {
        return Err(fail(
            "pre-open FIFO writer survived controlled cancellation",
        ));
    }

    let post = start_status_fifo_writer(&directory.join("post-status"), &directory)?;
    let mut post_cleanup = post.external_cleanup();
    super::wait_until(|| post.started().exists(), Duration::from_secs(5))?;
    post.allow_open()?;
    let mut reader = spawn_fifo_reader(
        &directory.join("post-status"),
        &directory.join("post-output"),
    )?;
    super::wait_until(|| post.opened().exists(), Duration::from_secs(5))?;
    let post_group = post.process_group();
    drop(post);
    let post_survived = crate::gc1_seal_units::group_is_live(post_group);
    post_cleanup.terminate_and_reap()?;
    reader.wait_success()?;
    if post_survived || !fs::read(directory.join("post-output"))?.is_empty() {
        return Err(fail("post-open FIFO cancellation did not close the reader"));
    }

    let normal = start_status_fifo_writer(&directory.join("normal-status"), &directory)?;
    super::wait_until(|| normal.started().exists(), Duration::from_secs(5))?;
    normal.allow_open()?;
    let mut normal_reader = spawn_fifo_reader(
        &directory.join("normal-status"),
        &directory.join("normal-output"),
    )?;
    super::wait_until(|| normal.opened().exists(), Duration::from_secs(5))?;
    let mut normal = normal;
    normal.release_and_wait()?;
    normal_reader.wait_success()?;
    if fs::read(directory.join("normal-output"))? != b"0\n" {
        return Err(fail("status FIFO did not deliver exact 0 newline bytes"));
    }
    scene.finish()
}

fn observe_fifo_open_without_reader(group: u32, opened: &std::path::Path) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if opened.exists() {
            return false;
        }
        let wait_channel = fs::read_to_string(format!("/proc/{group}/wchan")).unwrap_or_default();
        if wait_channel.contains("wait_for_partner") {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct AggregateStatusReadCase {
    verify_release: ReleaseMarker,
    selector: RunningSelector,
    clock: SupervisorClock,
    completed: String,
    gh_dir: PathBuf,
    group_log: String,
    scene: Scene,
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
    let verify_release = ReleaseMarker::new(gate.clone());
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
        verify_release,
        selector,
        clock,
        completed,
        gh_dir,
        group_log,
        scene,
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
    let mut release = clock.block_guarded(barrier)?;
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
    release.release()?;
    if first_status_ready {
        wait_until(|| !first_status.exists(), Duration::from_secs(10))?;
    }
    Ok(())
}
