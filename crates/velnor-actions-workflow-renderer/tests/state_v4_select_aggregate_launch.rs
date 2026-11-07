// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use velnor_actions_contract::workflow::cache_family::StateUnit;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_SELECT_MAX_PARALLELISM;

use super::{RunningSelector, wait_until};
use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_clock::{ReleaseMarker, SupervisorClock};
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{
    expect, expect_flag, group_is_live, produce_flag, selected_unit, unit_key,
};

#[path = "state_v4_select_launch_custody.rs"]
mod launch_custody;

use launch_custody::LaunchGuardianCleanup;

#[test]
fn expiry_during_launch_does_not_start_late_verifiers() -> Result<(), Box<dyn Error>> {
    let mut case = LaunchCase::start()?;
    let started = case.dir.join("launch-release.started");
    wait_until(|| started.exists(), Duration::from_secs(40))?;
    let group: u32 = fs::read_to_string(started)?.parse()?;
    let groups = fs::read_to_string(case.dir.join("group-log"))?;
    let first_group: u32 = groups
        .lines()
        .nth(2)
        .ok_or_else(|| fail("first unit was not cleaned"))?
        .parse()?;
    if PathBuf::from(format!("/proc/{first_group}")).exists() || group_is_live(first_group) {
        return Err(fail("previously successful unit was not gone and reaped"));
    }
    case.clock.advance("launch-expiry", 6)?;
    case.launch_release.release()?;
    if !case.selector.wait(Duration::from_secs(15))?.success() {
        return Err(fail("aggregate launch expiry failed the selector step"));
    }
    case.verify_release.release()?;
    let group_log = case.dir.join("group-log");
    if fs::read_to_string(&group_log)?.lines().count() < 35 {
        return Err(fail("not every launched guardian was recorded"));
    }
    super::aggregate_cleanup::assert_groups_reaped(
        group_log
            .to_str()
            .ok_or_else(|| fail("invalid group log path"))?,
    )?;
    let subjects = fs::read_to_string(case.dir.join("subjects")).unwrap_or_default();
    if subjects.contains(&case.keys[32]) {
        return Err(fail(
            "verification started after aggregate expiry during launch",
        ));
    }
    if PathBuf::from(format!("/proc/{group}")).exists() || group_is_live(group) {
        return Err(fail(
            "guardian launched across expiry was not gone and reaped",
        ));
    }
    case.assert_outputs()?;
    case.scene.finish()
}

#[test]
fn launch_case_releases_before_scene_removal_and_reaps_guardian() -> Result<(), Box<dyn Error>> {
    let hold = PathBuf::from(format!("/tmp/launch-drop-hold-{}", std::process::id()));
    drop(fs::remove_file(&hold));
    let mut hold_release = ReleaseMarker::new(hold.clone());
    let case = LaunchCase::start_with_hold(Some(&hold))?;
    let started = case.dir.join("launch-release.started");
    let launch_group = wait_until(|| started.exists(), Duration::from_secs(40))
        .ok()
        .and_then(|()| fs::read_to_string(&started).ok())
        .and_then(|contents| contents.trim().parse::<u32>().ok());
    let directory = case.dir.clone();
    let release = case.dir.join("launch-release");
    let mut cleanup = LaunchGuardianCleanup::new(case.dir.clone(), case.selector.child.id());
    let capture_error = cleanup.capture().err();
    let active_guardian_groups = cleanup.active_guardian_groups();
    let (sender, receiver) = mpsc::channel();
    let mut dropper = Some(thread::spawn(move || {
        drop(case);
        let _send_result = sender.send(());
    }));
    let release_observed = wait_until(|| release.exists(), Duration::from_secs(2)).is_ok();
    let scene_alive_before_selector_teardown = directory.exists();
    let hold_error = hold_release.release().err();
    drop(hold_release);
    let mut worker_finished = false;
    let mut drop_completed = false;
    match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => {
            worker_finished = true;
            drop_completed = true;
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => worker_finished = true,
        Err(mpsc::RecvTimeoutError::Timeout) => {}
    }
    let first_cleanup_error = cleanup.cleanup().err();
    if !worker_finished {
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {
                worker_finished = true;
                drop_completed = true;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => worker_finished = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    let worker_panicked = worker_finished
        && dropper
            .take()
            .is_some_and(|dropper| dropper.join().is_err());
    drop(dropper);
    let second_cleanup_error = cleanup.cleanup().err();
    let surviving_groups = cleanup.surviving_groups();
    drop(fs::remove_file(hold));
    if let Some(error) = capture_error
        .or(hold_error)
        .or(first_cleanup_error)
        .or(second_cleanup_error)
    {
        return Err(error);
    }
    if launch_group.is_none()
        || !cleanup.contains_group(launch_group.unwrap_or_default())
        || active_guardian_groups < STATE_SEAL_SELECT_MAX_PARALLELISM
        || !release_observed
        || !scene_alive_before_selector_teardown
        || !drop_completed
        || worker_panicked
        || !surviving_groups.is_empty()
    {
        return Err(fail(
            "launch custody ordering failed controlled cleanup assertions",
        ));
    }
    Ok(())
}

#[test]
fn launch_clock_block_releases_on_early_return() -> Result<(), Box<dyn Error>> {
    let mut case = LaunchCase::start()?;
    let started = case.dir.join("launch-release.started");
    wait_until(|| started.exists(), Duration::from_secs(40))?;
    let launch_clock_error = (|| -> Result<(), Box<dyn Error>> {
        let _clock_release = case.clock.block_guarded("launch-early-return")?;
        case.launch_release.release()?;
        case.clock
            .wait_for_block("launch-early-return", Duration::from_secs(10))?;
        case.clock.advance("launch-early-progress", 1)?;
        Err(fail("forced early return while launch clock was blocked"))
    })();
    let clock_released = case
        .clock
        .wait_for_advance("launch-early-progress", Duration::from_secs(2))
        .is_ok();
    if !clock_released {
        fs::write(
            case.dir
                .join("clock-block-release-control-launch-early-return"),
            "release\n",
        )?;
        case.clock
            .wait_for_advance("launch-early-progress", Duration::from_secs(10))?;
    }
    case.verify_release.release()?;
    if !case.selector.wait(Duration::from_secs(15))?.success() {
        return Err(fail(
            "launch clock release did not complete selector cleanup",
        ));
    }
    if !launch_clock_error
        .expect_err("launch clock fixture must exercise early return")
        .to_string()
        .contains("forced early return")
        || !clock_released
    {
        return Err(fail(
            "launch clock guard did not release after early return",
        ));
    }
    case.scene.finish()
}

struct LaunchCase {
    launch_release: ReleaseMarker,
    verify_release: ReleaseMarker,
    selector: RunningSelector,
    clock: SupervisorClock,
    dir: PathBuf,
    roots: Vec<String>,
    keys: Vec<String>,
    scene: Scene,
}

impl LaunchCase {
    fn start() -> Result<Self, Box<dyn Error>> {
        Self::start_with_hold(None)
    }

    fn start_with_hold(hold: Option<&std::path::Path>) -> Result<Self, Box<dyn Error>> {
        let scene = Scene::new("aggregate-launch-boundary", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let (roots, keys) = launch_candidates(&gh)?;
        let clock = SupervisorClock::install(&scene, &gh)?;
        let dir = PathBuf::from(gh.dir());
        let gate = dir.join("launch-release");
        let gate_value = gate.to_string_lossy().into_owned();
        let verify_gate = dir.join("verify-release").to_string_lossy().into_owned();
        let verify_started = dir.join("verify-started").to_string_lossy().into_owned();
        let group_log = dir.join("group-log").to_string_lossy().into_owned();
        let launch_inventory = dir.join("launch-inventory");
        fs::write(&launch_inventory, "")?;
        let launch_inventory_value = launch_inventory.to_string_lossy().into_owned();
        let python = clock
            .real_python()
            .to_str()
            .ok_or_else(|| fail("invalid Python path"))?;
        let units: Vec<_> = roots
            .iter()
            .map(|root| (StateUnit::Union, root.as_str()))
            .collect();
        let launch_release = ReleaseMarker::new(gate.clone());
        let verify_release = ReleaseMarker::new(PathBuf::from(&verify_gate));
        let mut extra = vec![
            ("VELNOR_STATE_GH_TIMEOUT", "60"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "120"),
            ("VELNOR_STATE_SELECT_DEADLINE", "5"),
            ("FAKE_REAL_PYTHON3", python),
            ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
            ("FAKE_SUPERVISOR_LAUNCH_GATE", &gate_value),
            ("FAKE_SUPERVISOR_LAUNCH_TAG", "u32"),
            ("FAKE_SUPERVISOR_LAUNCH_KEY", &keys[32]),
            ("FAKE_SUPERVISOR_GROUP_LOG", &group_log),
            ("FAKE_SUPERVISOR_LAUNCH_INVENTORY", &launch_inventory_value),
            ("FAKE_GH_VERIFY_GATE", &verify_gate),
            ("FAKE_GH_VERIFY_GATE_EXCEPT", &keys[0]),
            ("FAKE_GH_VERIFY_STARTED", &verify_started),
        ];
        if let Some(hold) = hold {
            extra.push((
                "FAKE_SUPERVISOR_LAUNCH_HOLD",
                hold.to_str()
                    .ok_or_else(|| fail("invalid launch hold path"))?,
            ));
        }
        let child = crate::gc1_seal_units::spawn_select_units(&scene, &gh, &units, &extra)?;
        let selector = RunningSelector::new(child, &gh);
        Ok(Self {
            launch_release,
            verify_release,
            selector,
            clock,
            dir,
            roots,
            keys,
            scene,
        })
    }

    fn assert_outputs(&self) -> Result<(), Box<dyn Error>> {
        expect(
            &selected_unit(&self.scene, StateUnit::Union, &self.roots[0])?,
            &self.keys[..1],
            "success observed before launch expiry",
        )?;
        expect_flag(
            &produce_flag(&self.scene, StateUnit::Union, &self.roots[0])?,
            "false",
            "success observed before launch expiry",
        )?;
        for root in &self.roots[1..] {
            expect(
                &selected_unit(&self.scene, StateUnit::Union, root)?,
                &[],
                "unfinished unit after launch expiry",
            )?;
            expect_flag(
                &produce_flag(&self.scene, StateUnit::Union, root)?,
                "true",
                "unfinished unit after launch expiry",
            )?;
        }
        Ok(())
    }
}

fn launch_candidates(gh: &Gh) -> Result<(Vec<String>, Vec<String>), Box<dyn Error>> {
    let roots: Vec<String> = (0..33)
        .map(|index| {
            if index == 0 {
                ".".into()
            } else {
                format!("r{index}")
            }
        })
        .collect();
    let keys = roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            unit_key(
                StateUnit::Union,
                root,
                index as u64,
                0x1000 + index as u64,
                1,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rows: Vec<_> = keys
        .iter()
        .map(|key| ("2026-11-04T03:00:00Z", MAIN, key.as_str()))
        .collect();
    gh.list(&rows)?;
    gh.attest(
        &keys
            .iter()
            .map(|key| Attestation::signed(key))
            .collect::<Vec<_>>(),
    )?;
    Ok((roots, keys))
}
