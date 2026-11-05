// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use velnor_actions_contract::workflow::cache_family::StateUnit;
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_LIST_LIMIT, STATE_SEAL_SELECT_LIST_LIMIT, STATE_SEAL_SELECT_PARALLELISM,
};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh, MAIN, REPO};
#[cfg(target_os = "linux")]
use crate::gc1_seal_units::hang_keys;
use crate::gc1_seal_units::{
    produce_flag, rendezvous, run_select_units, selected_unit, unit_key, unit_scope,
};

const T1: &str = "2026-11-04T01:00:00Z";
const T3: &str = "2026-11-04T03:00:00Z";

#[derive(Clone, Copy)]
enum Kind {
    Attested,
    ForgedUnattested,
    ForgedSigner,
}

fn expect(got: &[String], want: &[String], label: &str) -> Result<(), Box<dyn Error>> {
    if got == want {
        Ok(())
    } else {
        Err(fail(&format!("{label}: selected {got:?}, wanted {want:?}")))
    }
}

fn expect_flag(got: &str, want: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if got == want {
        Ok(())
    } else {
        Err(fail(&format!("{label}: produce {got}, wanted {want}")))
    }
}

fn at(second: usize) -> String {
    let total = 86_399 - second;
    format!(
        "2026-11-04T{:02}:{:02}:{:02}Z",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

fn rows_of(rows: &[(String, String)]) -> Vec<(&str, &str, &str)> {
    rows.iter()
        .map(|(created, key)| (created.as_str(), MAIN, key.as_str()))
        .collect()
}

#[test]
#[cfg(unix)]
fn plan_never_selects_another_units_attestation_for_a_forged_entry() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-units", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;

    let mut units: Vec<(StateUnit, String, Kind)> = vec![
        (StateUnit::Union, ".".to_owned(), Kind::Attested),
        (StateUnit::Union, "a".to_owned(), Kind::ForgedUnattested),
        (StateUnit::Union, "a-b".to_owned(), Kind::Attested),
        (StateUnit::Suite, "a".to_owned(), Kind::Attested),
        (StateUnit::Suite, "a-b".to_owned(), Kind::ForgedSigner),
    ];
    let cycle = [Kind::Attested, Kind::ForgedUnattested, Kind::ForgedSigner];
    for ordinal in 0..18 {
        units.push((
            StateUnit::Union,
            format!("r{ordinal:02}"),
            cycle[ordinal % cycle.len()],
        ));
    }
    if units.len() <= STATE_SEAL_SELECT_PARALLELISM {
        return Err(fail("the units must outnumber the parallelism"));
    }

    let mut rows: Vec<(String, String)> = Vec::new();
    let mut attested: Vec<Attestation> = Vec::new();
    let mut wanted: Vec<Vec<String>> = Vec::new();
    for (index, (unit, root, kind)) in units.iter().enumerate() {
        let ordinal = index as u64;
        let snapshot = 0x1000 + ordinal;
        match kind {
            Kind::Attested => {
                let entry = unit_key(*unit, root, ordinal, snapshot, 1)?;
                rows.push((T3.to_owned(), entry.clone()));
                attested.push(Attestation::signed(&entry));
                wanted.push(vec![entry]);
            }
            Kind::ForgedUnattested | Kind::ForgedSigner => {
                let forged = unit_key(*unit, root, ordinal, snapshot, 2)?;
                let older = unit_key(*unit, root, ordinal, 0x2000 + ordinal, 3)?;
                rows.push((T3.to_owned(), forged.clone()));
                rows.push((T1.to_owned(), older.clone()));
                attested.push(Attestation::signed(&older));
                if matches!(kind, Kind::ForgedSigner) {
                    attested.push(Attestation {
                        signer: format!("{REPO}/.github/workflows/release.yml"),
                        ..Attestation::signed(&forged)
                    });
                }
                wanted.push(vec![older]);
            }
        }
    }
    gh.list(&rows_of(&rows))?;
    gh.attest(&attested)?;
    rendezvous(&gh, STATE_SEAL_SELECT_PARALLELISM)?;

    let targets: Vec<(StateUnit, &str)> = units
        .iter()
        .map(|(unit, root, _)| (*unit, root.as_str()))
        .collect();
    run_select_units(&scene, &gh, &targets, &[])?;

    let mut mismatches: Vec<String> = Vec::new();
    for (index, (unit, root, kind)) in units.iter().enumerate() {
        let label = format!("{} {root}", unit_scope(*unit, root)?.prefix());
        let produce = if matches!(kind, Kind::Attested) {
            "false"
        } else {
            "true"
        };
        mismatches.extend(
            expect(&selected_unit(&scene, *unit, root)?, &wanted[index], &label)
                .err()
                .map(|error| error.to_string()),
        );
        mismatches.extend(
            expect_flag(&produce_flag(&scene, *unit, root)?, produce, &label)
                .err()
                .map(|error| error.to_string()),
        );
    }
    if !mismatches.is_empty() {
        return Err(fail(&mismatches.join("\n")));
    }
    scene.finish()
}

fn filler(count: usize, offset: usize) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    (0..count)
        .map(|index| {
            let key = unit_key(
                StateUnit::Union,
                "z",
                0,
                0x3000 + (offset + index) as u64,
                9,
            )?;
            Ok((at(1000 + (offset + index)), key))
        })
        .collect()
}

#[test]
#[cfg(unix)]
fn plan_selects_nothing_when_the_listing_may_be_truncated_or_unavailable()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-truncated", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let units = [(StateUnit::Union, "."), (StateUnit::Union, "b")];
    let first = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let second = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
    gh.attest(&[Attestation::signed(&first), Attestation::signed(&second)])?;

    let listing = |total: usize| -> Result<Vec<(String, String)>, Box<dyn Error>> {
        let mut rows = vec![(at(0), first.clone()), (at(1), second.clone())];
        rows.extend(filler(total - rows.len(), 0)?);
        Ok(rows)
    };

    gh.list(&rows_of(&listing(STATE_SEAL_SELECT_LIST_LIMIT - 1)?))?;
    run_select_units(&scene, &gh, &units, &[])?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&first),
        "999 rows, first unit",
    )?;
    expect(
        &selected_unit(&scene, StateUnit::Union, "b")?,
        std::slice::from_ref(&second),
        "999 rows, second unit",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "false",
        "999 rows",
    )?;

    gh.list(&rows_of(&listing(STATE_SEAL_SELECT_LIST_LIMIT)?))?;
    run_select_units(&scene, &gh, &units, &[])?;
    for (unit, root) in units {
        expect(
            &selected_unit(&scene, unit, root)?,
            &[],
            &format!("1000 rows, unit {root}"),
        )?;
        expect_flag(
            &produce_flag(&scene, unit, root)?,
            "true",
            &format!("1000 rows, unit {root}"),
        )?;
    }

    gh.list(&rows_of(&listing(STATE_SEAL_SELECT_LIST_LIMIT - 1)?))?;
    for mode in ["fail", "garbage"] {
        gh.mode(mode)?;
        run_select_units(&scene, &gh, &units, &[])?;
        for (unit, root) in units {
            expect(
                &selected_unit(&scene, unit, root)?,
                &[],
                &format!("listing {mode}, unit {root}"),
            )?;
        }
    }
    gh.mode("ok")?;
    run_select_units(&scene, &gh, &units, &[])?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&first),
        "listing restored",
    )?;
    scene.finish()
}

#[test]
#[cfg(target_os = "linux")]
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
#[cfg(unix)]
fn plan_caps_each_unit_at_its_own_newest_rows() -> Result<(), Box<dyn Error>> {
    let units = [(StateUnit::Union, "."), (StateUnit::Union, "b")];
    for (rank, selects) in [
        (STATE_SEAL_LIST_LIMIT, true),
        (STATE_SEAL_LIST_LIMIT + 1, false),
    ] {
        let scene = Scene::new("seal-select-cap", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let prefix = unit_scope(StateUnit::Union, ".")?.prefix();
        let other = unit_scope(StateUnit::Union, "b")?.prefix();
        let attested = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        let newest = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;

        let mut rows: Vec<(String, String)> = Vec::new();
        for position in 1..rank {
            rows.push((at(2 * position), format!("{prefix}s256-nothex{position}")));
        }
        rows.push((at(2 * rank), attested.clone()));
        rows.push((at(1), newest.clone()));
        for position in 1..=(STATE_SEAL_LIST_LIMIT + 50) {
            rows.push((
                at(2 * position + 1),
                format!("{other}s256-nothex{position}"),
            ));
        }
        gh.list(&rows_of(&rows))?;
        gh.attest(&[Attestation::signed(&attested), Attestation::signed(&newest)])?;
        run_select_units(&scene, &gh, &units, &[])?;

        let want: Vec<String> = if selects { vec![attested] } else { Vec::new() };
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &want,
            &format!("attested row at rank {rank}"),
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, "b")?,
            &[newest],
            &format!("neighbour unit with rank {rank}"),
        )?;
        scene.finish()?;
    }
    Ok(())
}
