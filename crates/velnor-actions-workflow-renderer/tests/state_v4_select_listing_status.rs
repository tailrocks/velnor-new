// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use velnor_actions_contract::workflow::cache_family::StateUnit;

use super::{RunningSelector, wait_until};
use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_clock::{ReleaseMarker, SupervisorClock};
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{expect, expect_flag, produce_flag, selected_unit, unit_key};

#[test]
fn release_marker_stays_armed_after_write_error() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("release-marker-write-error", crate::gc1_t53::scripts()?)?;
    let parent = scene.scratch.join("missing-release-parent");
    let marker = parent.join("release");
    let mut release = ReleaseMarker::new(marker.clone());
    if release.release().is_ok() {
        return Err(fail("release marker write unexpectedly succeeded"));
    }
    fs::create_dir_all(parent)?;
    drop(release);
    let marker_contents = match fs::read_to_string(&marker) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    scene.finish()?;
    if marker_contents.as_deref() != Some("release\n") {
        return Err(fail("release marker did not retry after a write error"));
    }
    Ok(())
}

#[test]
fn listing_status_observed_at_or_after_call_deadline_is_cold() -> Result<(), Box<dyn Error>> {
    for (label, advance) in [("before", 1), ("equal", 2), ("late", 3)] {
        listing_status_case(label, advance)?;
    }
    Ok(())
}

#[test]
fn dropped_clock_block_releases_supervisor_on_early_return() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("listing-clock-drop-release", crate::gc1_t53::scripts()?)?;
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
    let verify_gate = PathBuf::from(gh.dir()).join("verify-release");
    let verify_started = PathBuf::from(gh.dir()).join("verify-started");
    let verify_gate_value = verify_gate.to_string_lossy().into_owned();
    let verify_started_value = verify_started.to_string_lossy().into_owned();
    let verify_release = ReleaseMarker::new(verify_gate.clone());
    let mut selector = RunningSelector::new(
        crate::gc1_seal_units::spawn_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, ".")],
            &[
                ("VELNOR_STATE_GH_TIMEOUT", "10"),
                ("VELNOR_STATE_UNIT_TIMEOUT", "10"),
                ("VELNOR_STATE_SELECT_DEADLINE", "30"),
                ("FAKE_REAL_PYTHON3", python),
                ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
                ("FAKE_GH_VERIFY_GATE", &verify_gate_value),
                ("FAKE_GH_VERIFY_GATE_KEY", &key),
                ("FAKE_GH_VERIFY_STARTED", &verify_started_value),
            ],
        )?,
        &gh,
    );
    let mut verify_release = verify_release;
    wait_until(|| verify_started.exists(), Duration::from_secs(10))?;
    let early_error = (|| -> Result<(), Box<dyn Error>> {
        let _release = clock.block_guarded("early-return")?;
        clock.wait_for_block("early-return", Duration::from_secs(10))?;
        clock.advance("early-return-progress", 1)?;
        Err(fail("forced fixture error while the clock was blocked"))
    })();
    let release_observed = clock
        .wait_for_advance("early-return-progress", Duration::from_secs(2))
        .is_ok();
    if !release_observed {
        fs::write(
            PathBuf::from(gh.dir()).join("clock-block-release-control-early-return"),
            "release\n",
        )?;
        clock.wait_for_advance("early-return-progress", Duration::from_secs(10))?;
    }
    verify_release.release()?;
    if !selector.wait(Duration::from_secs(10))?.success() {
        return Err(fail("clock barrier drop did not release the selector"));
    }
    if !early_error
        .expect_err("the fixture must take the early-return path")
        .to_string()
        .contains("forced fixture error")
        || !release_observed
    {
        return Err(fail(
            "clock barrier guard did not release after early return",
        ));
    }
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        &[key],
        "clock barrier drop preserves the valid selector result",
    )?;
    scene.finish()
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
    let list_release = ReleaseMarker::new(gate.clone());
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
    let mut list_release = list_release;
    wait_until(|| started.exists(), Duration::from_secs(10))?;
    let mut release = clock.block_guarded(label)?;
    clock.wait_for_block(label, Duration::from_secs(10))?;
    list_release.release()?;
    let status_path = scene.runner.join("velnor/state-select/status-list");
    wait_until(|| status_path.exists(), Duration::from_secs(10))?;
    clock.advance(label, advance)?;
    release.release()?;
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
