// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use velnor_actions_contract::workflow::cache_family::StateUnit;

use super::{RunningSelector, wait_until};
use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_clock::ReleaseMarker;
use crate::gc1_seal_clock::SupervisorClock;
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{expect, expect_flag, produce_flag, selected_unit, unit_key};

#[test]
fn success_observed_before_expiry_survives_cleanup_after_expiry() -> Result<(), Box<dyn Error>> {
    let (prepared, cleanup_release, verify_release) = prepare_cleanup_case()?;
    let child = crate::gc1_seal_units::spawn_select_units(
        &prepared.scene,
        &prepared.gh,
        &[(StateUnit::Union, "."), (StateUnit::Union, "b")],
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "30"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
            ("VELNOR_STATE_SELECT_DEADLINE", "5"),
            ("FAKE_REAL_PYTHON3", &prepared.python),
            ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
            ("FAKE_SUPERVISOR_CLEANUP_GATE", &prepared.cleanup_gate),
            ("FAKE_SUPERVISOR_GROUP_LOG", &prepared.group_log),
            ("FAKE_GH_VERIFY_GATE", &prepared.verify_gate),
            ("FAKE_GH_VERIFY_GATE_KEY", &prepared.second),
            ("FAKE_GH_VERIFY_STARTED", &prepared.verify_started),
        ],
    )?;
    let mut selector = RunningSelector::new(child, &prepared.gh);
    let cleanup_release = cleanup_release;
    let mut verify_release = verify_release;
    run_cleanup_boundary(
        &prepared,
        &mut selector,
        cleanup_release,
        &mut verify_release,
    )?;
    prepared.scene.finish()
}

fn run_cleanup_boundary(
    prepared: &CleanupPreparation,
    selector: &mut RunningSelector,
    cleanup_release: ReleaseMarker,
    verify_release: &mut ReleaseMarker,
) -> Result<(), Box<dyn Error>> {
    wait_until(
        || PathBuf::from(&prepared.verify_started).exists(),
        Duration::from_secs(10),
    )?;
    wait_until(
        || PathBuf::from(format!("{}.blocked", prepared.cleanup_gate)).exists(),
        Duration::from_secs(10),
    )?;
    let cleanup_clock = &prepared.clock;
    let cleanup_error = (move || -> Result<(), Box<dyn Error>> {
        let _cleanup_release = cleanup_release;
        cleanup_clock.advance("cleanup-crosses-expiry", 6)?;
        Err(fail("forced early return while cleanup was held"))
    })();
    let cleanup_released = wait_until(
        || PathBuf::from(&prepared.cleanup_gate).exists(),
        Duration::from_secs(2),
    )
    .is_ok();
    if !cleanup_released {
        fs::write(&prepared.cleanup_gate, "release\n")?;
    }
    prepared
        .clock
        .wait_for_advance("cleanup-crosses-expiry", Duration::from_secs(10))?;
    if !selector.wait(Duration::from_secs(10))?.success() {
        return Err(fail("aggregate cleanup boundary failed the selector step"));
    }
    verify_release.release()?;
    assert_groups_reaped(&prepared.group_log)?;
    let Err(cleanup_error) = cleanup_error else {
        return Err(fail("cleanup fixture must exercise the early-return path"));
    };
    if !cleanup_error
        .to_string()
        .contains("forced early return while cleanup was held")
        || !cleanup_released
    {
        return Err(fail(
            "cleanup release guard did not release after early return",
        ));
    }
    expect(
        &selected_unit(&prepared.scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&prepared.first),
        "accepted before cleanup expiry",
    )?;
    expect_flag(
        &produce_flag(&prepared.scene, StateUnit::Union, ".")?,
        "false",
        "accepted before cleanup expiry",
    )?;
    expect(
        &selected_unit(&prepared.scene, StateUnit::Union, "b")?,
        &[],
        "unfinished at aggregate expiry",
    )?;
    expect_flag(
        &produce_flag(&prepared.scene, StateUnit::Union, "b")?,
        "true",
        "unfinished at aggregate expiry",
    )?;
    Ok(())
}

struct CleanupPreparation {
    scene: Scene,
    gh: Gh,
    clock: SupervisorClock,
    first: String,
    second: String,
    cleanup_gate: String,
    verify_gate: String,
    group_log: String,
    verify_started: String,
    python: String,
}

fn prepare_cleanup_case()
-> Result<(CleanupPreparation, ReleaseMarker, ReleaseMarker), Box<dyn Error>> {
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
        .ok_or_else(|| fail("invalid Python path"))?
        .to_owned();
    let cleanup_release = ReleaseMarker::new(PathBuf::from(cleanup_gate.as_str()));
    let verify_release = ReleaseMarker::new(PathBuf::from(verify_gate.as_str()));
    Ok((
        CleanupPreparation {
            scene,
            gh,
            clock,
            first,
            second,
            cleanup_gate,
            verify_gate,
            group_log,
            verify_started,
            python,
        },
        cleanup_release,
        verify_release,
    ))
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
