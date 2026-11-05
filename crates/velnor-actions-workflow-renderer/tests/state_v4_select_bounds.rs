// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::time::{Duration, Instant};

use velnor_actions_contract::workflow::cache_family::StateUnit;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_SELECT_MAX_PARALLELISM;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{
    at, expect, hang_keys, rows_of, run_select_units, selected_unit, unit_key,
};

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
            ("VELNOR_STATE_GH_TIMEOUT", "60"),
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
    scene.finish()
}
