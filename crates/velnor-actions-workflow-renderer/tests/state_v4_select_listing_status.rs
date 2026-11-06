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
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{expect, expect_flag, produce_flag, selected_unit, unit_key};

#[test]
fn listing_status_observed_at_or_after_call_deadline_is_cold() -> Result<(), Box<dyn Error>> {
    for (label, advance) in [("before", 1), ("equal", 2), ("late", 3)] {
        listing_status_case(label, advance)?;
    }
    Ok(())
}

fn listing_status_case(label: &str, advance: u64) -> Result<(), Box<dyn Error>> {
    eprintln!("X101 case listing={label}");
    let scene = Scene::new(
        &format!("listing-status-{label}"),
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    gh.list(&crate::gc1_seal_units::rows_of(&[(
        crate::gc1_seal_units::at(0),
        key.clone(),
    )]))?;
    gh.attest(&[Attestation::signed(&key)])?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    let python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;
    let dir = PathBuf::from(gh.dir());
    let gate = dir.join("list-release");
    let started = dir.join("list-started");
    let gate_value = gate.to_string_lossy().into_owned();
    let started_value = started.to_string_lossy().into_owned();
    let mut selector = RunningSelector::new(
        crate::gc1_seal_units::spawn_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, ".")],
            &[
                ("VELNOR_STATE_GH_TIMEOUT", "2"),
                ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
                ("VELNOR_STATE_SELECT_DEADLINE", "30"),
                ("FAKE_REAL_PYTHON3", python),
                ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
                ("FAKE_GH_LIST_GATE", &gate_value),
                ("FAKE_GH_LIST_STARTED", &started_value),
            ],
        )?,
        &gh,
    );
    wait_until(|| started.exists(), Duration::from_secs(10))?;
    clock.block(label)?;
    clock.wait_for_block(label, Duration::from_secs(10))?;
    fs::write(&gate, "release\n")?;
    let status_path = scene.runner.join("velnor/state-select/status-list");
    wait_until(|| status_path.exists(), Duration::from_secs(10))?;
    clock.advance(label, advance)?;
    clock.release_block(label)?;
    clock.wait_for_advance(label, Duration::from_secs(10))?;
    if !selector.wait(Duration::from_secs(10))?.success() {
        return Err(fail("listing status case failed the selector step"));
    }
    let selected = if advance < 2 { vec![key] } else { vec![] };
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        &selected,
        "listing status deadline",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        if advance < 2 { "false" } else { "true" },
        "listing status deadline",
    )?;
    if advance >= 2 && dir.join("verify-pids").exists() {
        return Err(fail("verification started after listing status deadline"));
    }
    scene.finish()
}
