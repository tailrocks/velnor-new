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
use crate::gc1_seal_units::{expect, expect_flag, produce_flag, selected_unit, unit_key};

#[test]
fn success_observed_before_expiry_survives_cleanup_after_expiry() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("aggregate-cleanup-boundary", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let first = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let second = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
    gh.list(&[
        ("2026-11-04T03:00:00Z", MAIN, &first),
        ("2026-11-04T02:00:00Z", MAIN, &second),
    ])?;
    gh.attest(&[Attestation::signed(&first), Attestation::signed(&second)])?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    let dir = PathBuf::from(gh.dir());
    let cleanup_gate = dir.join("cleanup-release").to_string_lossy().into_owned();
    let verify_gate = dir.join("verify-release").to_string_lossy().into_owned();
    let group_log = dir.join("group-log").to_string_lossy().into_owned();
    let verify_started = dir.join("verify-started").to_string_lossy().into_owned();
    let python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;
    let mut selector = RunningSelector::new(
        crate::gc1_seal_units::spawn_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, "."), (StateUnit::Union, "b")],
            &[
                ("VELNOR_STATE_GH_TIMEOUT", "30"),
                ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
                ("VELNOR_STATE_SELECT_DEADLINE", "5"),
                ("FAKE_REAL_PYTHON3", python),
                ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
                ("FAKE_SUPERVISOR_CLEANUP_GATE", &cleanup_gate),
                ("FAKE_SUPERVISOR_GROUP_LOG", &group_log),
                ("FAKE_GH_VERIFY_GATE", &verify_gate),
                ("FAKE_GH_VERIFY_GATE_KEY", &second),
                ("FAKE_GH_VERIFY_STARTED", &verify_started),
            ],
        )?,
        &gh,
    );
    let _release_on_drop = CleanupGate(PathBuf::from(&cleanup_gate));
    wait_until(
        || PathBuf::from(&verify_started).exists(),
        Duration::from_secs(10),
    )?;
    wait_until(
        || PathBuf::from(format!("{cleanup_gate}.blocked")).exists(),
        Duration::from_secs(10),
    )?;
    clock.advance("cleanup-crosses-expiry", 6)?;
    fs::write(&cleanup_gate, "release\n")?;
    clock.wait_for_advance("cleanup-crosses-expiry", Duration::from_secs(10))?;
    if !selector.wait(Duration::from_secs(10))?.success() {
        return Err(fail("aggregate cleanup boundary failed the selector step"));
    }
    assert_groups_reaped(&group_log)?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&first),
        "accepted before cleanup expiry",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "false",
        "accepted before cleanup expiry",
    )?;
    expect(
        &selected_unit(&scene, StateUnit::Union, "b")?,
        &[],
        "unfinished at aggregate expiry",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, "b")?,
        "true",
        "unfinished at aggregate expiry",
    )?;
    scene.finish()
}

struct CleanupGate(PathBuf);

impl Drop for CleanupGate {
    fn drop(&mut self) {
        drop(fs::write(&self.0, "release\n"));
    }
}

pub(super) fn assert_groups_reaped(log: &str) -> Result<(), Box<dyn Error>> {
    let groups = fs::read_to_string(log)?;
    let groups: Vec<u32> = groups.lines().map(str::parse).collect::<Result<_, _>>()?;
    if groups.len() < 4 {
        return Err(fail("expected listing, probe and both unit groups"));
    }
    for group in groups {
        if PathBuf::from(format!("/proc/{group}")).exists()
            || crate::gc1_seal_units::group_is_live(group)
        {
            return Err(fail("selector group was not gone and reaped"));
        }
    }
    Ok(())
}
