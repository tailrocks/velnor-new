// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{
    expect, expect_flag, produce_flag, run_select_units_with_timeout, selected_unit, unit_key,
};

fn override_listing(
    gh: &Gh,
    rows: &[(String, String, String, String)],
) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    for (created, key, id, size) in rows {
        text.push_str(created);
        text.push('\t');
        text.push_str(key);
        text.push('\t');
        text.push_str(id);
        text.push('\t');
        text.push_str(size);
        text.push('\n');
    }
    std::fs::write(Path::new(&gh.dir()).join("list-output-override"), text)?;
    Ok(())
}

fn row(created: &str, key: String) -> (String, String, String, String) {
    (
        created.to_owned(),
        key,
        "7001".to_owned(),
        "4096".to_owned(),
    )
}

#[test]
fn generated_deadlines_accept_supported_timeouts_and_reject_untrusted_expansion()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new(
        "seal-select-generated-deadlines",
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let units = [(StateUnit::Union, ".")];
    let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    override_listing(&gh, &[row("2026-11-04T03:00:00Z", key.clone())])?;
    gh.attest(&[Attestation::signed(&key)])?;

    for minutes in [121, 360] {
        let timeout = JobTimeout::new(minutes)?;
        let generated = u64::from(minutes) * 30;
        run_select_units_with_timeout(&scene, &gh, &units, &[], timeout)?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            std::slice::from_ref(&key),
            "supported generated deadline",
        )?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, ".")?,
            "false",
            "supported generated deadline",
        )?;
        run_select_units_with_timeout(
            &scene,
            &gh,
            &units,
            &[("VELNOR_STATE_SELECT_DEADLINE", &generated.to_string())],
            timeout,
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            std::slice::from_ref(&key),
            "deadline at trusted ceiling",
        )?;
        let above = (generated + 1).to_string();
        run_select_units_with_timeout(
            &scene,
            &gh,
            &units,
            &[
                ("VELNOR_STATE_SELECT_DEADLINE", &above),
                ("VELNOR_STATE_SELECT_MAX_DEADLINE", "999999"),
            ],
            timeout,
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &[],
            "deadline above trusted ceiling",
        )?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, ".")?,
            "true",
            "deadline above ceiling",
        )?;
    }

    scene.finish()
}

#[test]
fn selector_deadline_keeps_default_ceiling_and_rejects_invalid_values() -> Result<(), Box<dyn Error>>
{
    let scene = Scene::new(
        "seal-select-default-deadline-ceiling",
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let units = [(StateUnit::Union, ".")];
    let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    override_listing(&gh, &[row("2026-11-04T03:00:00Z", key.clone())])?;
    gh.attest(&[Attestation::signed(&key)])?;
    let timeout = JobTimeout::new(10)?;
    run_select_units_with_timeout(
        &scene,
        &gh,
        &units,
        &[("VELNOR_STATE_SELECT_DEADLINE", "3600")],
        timeout,
    )?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&key),
        "default ceiling equality",
    )?;
    for deadline in ["3601", "invalid", "NaN", "Infinity", "0", "-1"] {
        run_select_units_with_timeout(
            &scene,
            &gh,
            &units,
            &[("VELNOR_STATE_SELECT_DEADLINE", deadline)],
            timeout,
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &[],
            "invalid generated deadline",
        )?;
    }
    let verified = std::fs::read_to_string(Path::new(&gh.dir()).join("verify-pids"))?;
    if verified.lines().count() != 1 {
        return Err(fail(
            "invalid or above-ceiling deadlines started verification",
        ));
    }
    scene.finish()
}

#[test]
fn timestamps_sort_by_full_rfc3339_instant_without_truncating_fraction()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-timestamp-order", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let roots = [".", "offset", "tie"];
    let units = roots.map(|root| (StateUnit::Union, root));
    let keys = roots
        .iter()
        .map(|root| {
            (0..3)
                .map(|index| unit_key(StateUnit::Union, root, 0, 0x1000, index + 1))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rows = vec![
        row("2026-11-04T00:00:00.1Z", keys[0][2].clone()),
        row("2026-11-04T00:00:00.200000000Z", keys[0][0].clone()),
        row("2026-11-04T00:00:00.10000000000000001Z", keys[0][1].clone()),
        row("2026-11-03T23:59:59.9Z", keys[1][1].clone()),
        row("2026-11-04T00:30:00+01:00", keys[1][2].clone()),
        row("2026-11-04T00:00:00Z", keys[1][0].clone()),
        row("2026-11-04T00:00:00Z", keys[2][1].clone()),
        row("2026-11-04T02:00:00+02:00", keys[2][0].clone()),
        row("2026-11-03T23:59:59Z", keys[2][2].clone()),
    ];
    override_listing(&gh, &rows)?;
    let attestations = keys
        .iter()
        .flatten()
        .map(|key| Attestation::signed(key))
        .collect::<Vec<_>>();
    gh.attest(&attestations)?;
    run_select_units_with_timeout(&scene, &gh, &units, &[], JobTimeout::PLAN)?;

    expect(
        &selected_unit(&scene, units[0].0, units[0].1)?,
        &keys[0],
        "fraction precision and ordering",
    )?;
    expect(
        &selected_unit(&scene, units[1].0, units[1].1)?,
        &[keys[1][0].clone(), keys[1][1].clone(), keys[1][2].clone()],
        "offset chronology across date",
    )?;
    let mut tie = [keys[2][0].clone(), keys[2][1].clone()];
    tie.sort_by(|left, right| right.cmp(left));
    expect(
        &selected_unit(&scene, units[2].0, units[2].1)?,
        &[tie[0].clone(), tie[1].clone(), keys[2][2].clone()],
        "equal instant deterministic order",
    )?;
    scene.finish()
}

#[test]
fn malformed_timestamp_makes_the_complete_listing_cold_before_verification()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new(
        "seal-select-malformed-timestamps",
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let unit = (StateUnit::Union, ".");
    let key = unit_key(unit.0, unit.1, 0, 0x1000, 1)?;
    gh.attest(&[Attestation::signed(&key)])?;
    for malformed in [
        "2026-11-04T00:00:00",
        "2026-11-04T00:00:00.Z",
        "2026-11-04T00:00:00+24:00",
        "2026-02-30T00:00:00Z",
        "not-a-time",
    ] {
        override_listing(
            &gh,
            &[
                row("2026-11-04T00:00:01Z", key.clone()),
                row(malformed, unit_key(unit.0, unit.1, 0, 0x1000, 2)?),
            ],
        )?;
        run_select_units_with_timeout(&scene, &gh, &[unit], &[], JobTimeout::PLAN)?;
        expect(
            &selected_unit(&scene, unit.0, unit.1)?,
            &[],
            "malformed timestamp cold result",
        )?;
        expect_flag(
            &produce_flag(&scene, unit.0, unit.1)?,
            "true",
            "malformed timestamp",
        )?;
    }
    if Path::new(&gh.dir()).join("verify-pids").exists() {
        return Err(fail("malformed listing started an attestation verifier"));
    }
    scene.finish()
}

#[test]
fn exact_unit_rows_alone_consume_the_newest_hundred_cap() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-exact-unit-cap", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let a = (StateUnit::Union, "a");
    let ab = (StateUnit::Union, "a-b");
    let mut rows = Vec::new();
    let mut attested = Vec::new();
    for index in 0..101 {
        let key = unit_key(a.0, a.1, 0, 0x1000, index + 1)?;
        let mut cache_row = row(
            &format!("2026-11-04T00:{:02}:{:02}Z", index / 60, index % 60),
            key.clone(),
        );
        if index == 1 {
            cache_row.2 = "0007001".to_owned();
            cache_row.3 = "0004096".to_owned();
            attested.push(Attestation::bound_text(&key, "0007001", "0004096"));
        } else if index == 0 {
            attested.push(Attestation::signed(&key));
        }
        rows.push(cache_row);
    }
    for index in 0..150 {
        let key = unit_key(ab.0, ab.1, 1, 0x2000, index + 1)?;
        rows.push(row(
            &format!("2026-11-04T01:{:02}:{:02}Z", index / 60, index % 60),
            key.clone(),
        ));
        if index == 149 {
            attested.push(Attestation::signed(&key));
        }
    }
    rows.push(row(
        "2026-11-04T02:00:00Z",
        unit_key(StateUnit::Union, "irrelevant", 2, 0x3000, 1)?,
    ));
    override_listing(&gh, &rows)?;
    gh.attest(&attested)?;
    run_select_units_with_timeout(&scene, &gh, &[a, ab], &[], JobTimeout::PLAN)?;

    let accepted = unit_key(a.0, a.1, 0, 0x1000, 2)?;
    let excluded = unit_key(a.0, a.1, 0, 0x1000, 1)?;
    let latest_ab = unit_key(ab.0, ab.1, 1, 0x2000, 150)?;
    expect(
        &selected_unit(&scene, a.0, a.1)?,
        std::slice::from_ref(&accepted),
        "exact-unit 100th row",
    )?;
    expect(
        &selected_unit(&scene, ab.0, ab.1)?,
        &[latest_ab],
        "prefix-related unit cap",
    )?;
    if std::fs::read_to_string(Path::new(&gh.dir()).join("subjects"))?.contains(&excluded) {
        return Err(fail("the 101st exact-unit row was verified"));
    }
    let subject = std::fs::read_to_string(Path::new(&gh.dir()).join("subjects"))?;
    if !subject.contains(&format!("{accepted} 0007001 0004096\n")) {
        return Err(fail(
            "the attestation subject did not preserve cache ID and size text",
        ));
    }
    scene.finish()
}
