// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use velnor_actions_contract::workflow::cache_family::StateUnit;

use super::{RunningSelector, wait_until};
use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_clock::SupervisorClock;
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{
    expect, expect_flag, group_is_live, produce_flag, selected_unit, unit_key,
};

#[test]
fn expiry_during_launch_does_not_start_late_verifiers() -> Result<(), Box<dyn Error>> {
    let mut case = LaunchCase::start()?;
    let _release_on_drop = LaunchGate(case.gate.clone());
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
    fs::write(&case.gate, "release\n")?;
    if !case.selector.wait(Duration::from_secs(15))?.success() {
        return Err(fail("aggregate launch expiry failed the selector step"));
    }
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

struct LaunchCase {
    scene: Scene,
    clock: SupervisorClock,
    selector: RunningSelector,
    dir: PathBuf,
    gate: PathBuf,
    roots: Vec<String>,
    keys: Vec<String>,
}

impl LaunchCase {
    fn start() -> Result<Self, Box<dyn Error>> {
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
        let python = clock
            .real_python()
            .to_str()
            .ok_or_else(|| fail("invalid Python path"))?;
        let units: Vec<_> = roots
            .iter()
            .map(|root| (StateUnit::Union, root.as_str()))
            .collect();
        let selector = RunningSelector::new(
            crate::gc1_seal_units::spawn_select_units(
                &scene,
                &gh,
                &units,
                &[
                    ("VELNOR_STATE_GH_TIMEOUT", "60"),
                    ("VELNOR_STATE_UNIT_TIMEOUT", "120"),
                    ("VELNOR_STATE_SELECT_DEADLINE", "5"),
                    ("FAKE_REAL_PYTHON3", python),
                    ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
                    ("FAKE_SUPERVISOR_LAUNCH_GATE", &gate_value),
                    ("FAKE_SUPERVISOR_LAUNCH_TAG", "u32"),
                    ("FAKE_SUPERVISOR_LAUNCH_KEY", &keys[32]),
                    ("FAKE_SUPERVISOR_GROUP_LOG", &group_log),
                    ("FAKE_GH_VERIFY_GATE", &verify_gate),
                    ("FAKE_GH_VERIFY_GATE_EXCEPT", &keys[0]),
                    ("FAKE_GH_VERIFY_STARTED", &verify_started),
                ],
            )?,
            &gh,
        );
        Ok(Self {
            scene,
            clock,
            selector,
            dir,
            gate,
            roots,
            keys,
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

struct LaunchGate(PathBuf);
impl Drop for LaunchGate {
    fn drop(&mut self) {
        drop(fs::write(&self.0, "release\n"));
    }
}
