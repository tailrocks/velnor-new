// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::time::{Duration, Instant};

use velnor_actions_contract::workflow::cache_family::StateUnit;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_SELECT_MAX_PARALLELISM;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh};
#[cfg(unix)]
use crate::gc1_seal_units::expect_flag;
use crate::gc1_seal_units::{
    at, expect, hang_keys, rows_of, run_select_units, selected_unit, unit_key,
};
#[cfg(target_os = "linux")]
use crate::gc1_seal_units::{hung_processes_gone, process_is_running};

#[test]
fn plan_gives_a_hung_unit_no_selection_and_leaves_the_others() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-hung", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let units = [
        (StateUnit::Union, "h"),
        (StateUnit::Union, "."),
        (StateUnit::Union, "a"),
        (StateUnit::Suite, "a"),
    ];
    let verified = unit_key(StateUnit::Union, "h", 0, 0x1000, 1)?;
    let hung = unit_key(StateUnit::Union, "h", 0, 0x1000, 2)?;
    let behind = unit_key(StateUnit::Union, "h", 0, 0x1000, 3)?;
    let mut rows = vec![
        (at(0), verified.clone()),
        (at(1), hung.clone()),
        (at(2), behind.clone()),
    ];
    let mut attested = vec![
        Attestation::signed(&verified),
        Attestation::signed(&hung),
        Attestation::signed(&behind),
    ];
    let mut wanted = vec![Vec::new()];
    for (index, (unit, root)) in units.iter().enumerate().skip(1) {
        let entry = unit_key(*unit, root, index as u64, 0x1000 + index as u64, 1)?;
        rows.push((at(10 + index), entry.clone()));
        attested.push(Attestation::signed(&entry));
        wanted.push(vec![entry]);
    }
    gh.list(&rows_of(&rows))?;
    gh.attest(&attested)?;
    hang_keys(&gh, &[&hung])?;

    let started = Instant::now();
    run_select_units(
        &scene,
        &gh,
        &units,
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "30"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "3"),
        ],
    )?;
    let elapsed = started.elapsed();
    for (index, (unit, root)) in units.iter().enumerate() {
        expect(
            &selected_unit(&scene, *unit, root)?,
            &wanted[index],
            &format!("unit {root} with a hung neighbour"),
        )?;
    }
    if elapsed > Duration::from_secs(8) {
        return Err(fail(&format!(
            "the hung unit held the step for {elapsed:?}"
        )));
    }
    scene.finish()
}

#[test]
fn plan_stays_within_its_aggregate_deadline_when_every_verifier_hangs() -> Result<(), Box<dyn Error>>
{
    let scene = Scene::new("seal-select-aggregate", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let roots: Vec<String> = (0..STATE_SEAL_SELECT_MAX_PARALLELISM + 2)
        .map(|ordinal| format!("u{ordinal:02}"))
        .collect();
    let units: Vec<(StateUnit, &str)> = roots
        .iter()
        .map(|root| (StateUnit::Union, root.as_str()))
        .collect();

    let mut rows: Vec<(String, String)> = Vec::new();
    let mut attested: Vec<Attestation> = Vec::new();
    let mut hung: Vec<String> = Vec::new();
    for (index, (unit, root)) in units.iter().enumerate() {
        let ordinal = index as u64;
        let verified = unit_key(*unit, root, ordinal, 0x1000 + ordinal, 1)?;
        let blocked = unit_key(*unit, root, ordinal, 0x1000 + ordinal, 2)?;
        rows.push((at(2 * index), verified.clone()));
        rows.push((at(2 * index + 1), blocked.clone()));
        attested.push(Attestation::signed(&verified));
        attested.push(Attestation::signed(&blocked));
        hung.push(blocked);
    }
    gh.list(&rows_of(&rows))?;
    gh.attest(&attested)?;
    let hung_refs: Vec<&str> = hung.iter().map(String::as_str).collect();
    hang_keys(&gh, &hung_refs)?;

    let started = Instant::now();
    run_select_units(
        &scene,
        &gh,
        &units,
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "120"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "60"),
            ("VELNOR_STATE_SELECT_DEADLINE", "3"),
        ],
    )?;
    let elapsed = started.elapsed();
    for (unit, root) in &units {
        expect(
            &selected_unit(&scene, *unit, root)?,
            &[],
            &format!("unit {root} under a hanging verifier"),
        )?;
    }
    if elapsed > Duration::from_secs(8) {
        return Err(fail(&format!(
            "the select outlived its aggregate deadline: {elapsed:?}"
        )));
    }
    #[cfg(target_os = "linux")]
    hung_processes_gone(&gh)?;
    scene.finish()
}

#[test]
#[cfg(target_os = "linux")]
fn state_v4_select_listing_deadline_starts_no_verifier() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-list-hang", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let units = [(StateUnit::Union, "."), (StateUnit::Union, "b")];
    let first = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let second = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
    gh.list(&rows_of(&[(at(0), first.clone()), (at(1), second.clone())]))?;
    gh.attest(&[Attestation::signed(&first), Attestation::signed(&second)])?;
    gh.mode("list-hang")?;

    let started = Instant::now();
    run_select_units(
        &scene,
        &gh,
        &units,
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "30"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
            ("VELNOR_STATE_SELECT_DEADLINE", "2"),
        ],
    )?;
    let elapsed = started.elapsed();

    for (unit, root) in units {
        expect(
            &selected_unit(&scene, unit, root)?,
            &[],
            &format!("listing hang for {root}"),
        )?;
    }
    let gh_dir = gh.dir();
    let dir = std::path::Path::new(&gh_dir);
    if dir.join("verify-pids").exists() {
        return Err(fail("the selector started verification after listing hung"));
    }
    let list_pid: u32 = std::fs::read_to_string(dir.join("list-pid"))?
        .trim()
        .parse()?;
    let end = Instant::now() + Duration::from_secs(2);
    while process_is_running(list_pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    if process_is_running(list_pid) {
        return Err(fail(
            "the cache-list process remained alive after its deadline",
        ));
    }
    if elapsed > Duration::from_secs(4) {
        return Err(fail(&format!(
            "the listing exceeded its deadline: {elapsed:?}"
        )));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn state_v4_select_supervisor_rejects_invalid_bounds_and_missing_python()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-supervisor-inputs", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let units = [(StateUnit::Union, ".")];
    assert_valid_supervisor_bounds(&scene, &gh, &units)?;

    for (name, value) in [
        ("VELNOR_STATE_SELECT_DEADLINE", "invalid"),
        ("VELNOR_STATE_SELECT_DEADLINE", "NaN"),
        ("VELNOR_STATE_SELECT_DEADLINE", "Infinity"),
        ("VELNOR_STATE_SELECT_DEADLINE", "0"),
        ("VELNOR_STATE_SELECT_DEADLINE", "-1"),
        ("VELNOR_STATE_SELECT_DEADLINE", "3601"),
        ("VELNOR_STATE_UNIT_TIMEOUT", "invalid"),
        ("VELNOR_STATE_UNIT_TIMEOUT", "NaN"),
        ("VELNOR_STATE_UNIT_TIMEOUT", "Infinity"),
        ("VELNOR_STATE_UNIT_TIMEOUT", "0"),
        ("VELNOR_STATE_UNIT_TIMEOUT", "-1"),
        ("VELNOR_STATE_UNIT_TIMEOUT", "3601"),
    ] {
        run_select_units(
            &scene,
            &gh,
            &units,
            &[("VELNOR_STATE_GH_TIMEOUT", "30"), (name, value)],
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &[],
            &format!("invalid {name}={value}"),
        )?;
        expect_flag(
            &crate::gc1_seal_units::produce_flag(&scene, StateUnit::Union, ".")?,
            "true",
            &format!("invalid {name}={value}"),
        )?;
    }

    let path_without_python =
        scene.restricted_path(&["sh", "bash", "cat", "mkdir", "rm", "sort", "head"])?;
    let gh_bin = std::path::Path::new(&gh.dir())
        .parent()
        .ok_or_else(|| fail("fake GH directory has no parent"))?
        .join("bin");
    let path_without_python = format!("{path_without_python}:{}", gh_bin.display());
    run_select_units(
        &scene,
        &gh,
        &units,
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "30"),
            ("PATH", path_without_python.as_str()),
        ],
    )?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        &[],
        "missing Python",
    )?;

    scene.install_shim("python3", "#!/bin/sh\nexit 17\n")?;
    run_select_units(&scene, &gh, &units, &[("VELNOR_STATE_GH_TIMEOUT", "30")])?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        &[],
        "failed Python",
    )?;
    expect_flag(
        &crate::gc1_seal_units::produce_flag(&scene, StateUnit::Union, ".")?,
        "true",
        "failed Python",
    )?;
    scene.finish()
}

#[cfg(unix)]
fn assert_valid_supervisor_bounds(
    scene: &Scene,
    gh: &Gh,
    units: &[(StateUnit, &str)],
) -> Result<(), Box<dyn Error>> {
    let valid = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    gh.list(&rows_of(&[(at(0), valid.clone())]))?;
    gh.attest(&[Attestation::signed(&valid)])?;
    run_select_units(
        scene,
        gh,
        units,
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "30"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "2"),
            ("VELNOR_STATE_SELECT_DEADLINE", "4"),
        ],
    )?;
    expect(
        &selected_unit(scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&valid),
        "valid supervisor bounds",
    )
}
