// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::workflow::StepKind;
use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, ROOT_SLUG_MAX_BYTES, RootSlug, StateScope, StateTrust, StateUnit,
};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{
    expect, expect_flag, produce_flag, rows_of, run_select_units, selected_unit, unit_compat,
    unit_key, unit_scope, unit_snapshot,
};

use super::{T3, no_publication_or_network, scene};

fn typed_record(unit: StateUnit, root: &str) -> Result<String, Box<dyn Error>> {
    let step = state_select_step(
        &[SealRoot {
            unit,
            slug: RootSlug::from_root(root)?,
        }],
        JobTimeout::PLAN,
    )?;
    let StepKind::Shell { env, .. } = step.kind else {
        return Err(fail("selector step is not a shell step"));
    };
    env.get("VELNOR_STATE_SELECT_UNIT_R0")
        .cloned()
        .ok_or_else(|| fail("selector static unit record is absent"))
}

fn record_fields(record: &str) -> Result<(&str, &str, &str), Box<dyn Error>> {
    let fields: Vec<_> = record.split(' ').collect();
    if fields.len() != 3 {
        return Err(fail("typed selector record does not have three fields"));
    }
    Ok((fields[0], fields[1], fields[2]))
}

fn expect_unavailable_dynamic_metadata(
    scene: &Scene,
    gh: &Gh,
    name: &str,
    value: &str,
    empty_field: usize,
) -> Result<(), Box<dyn Error>> {
    run_select_units(scene, gh, &[(StateUnit::Union, ".")], &[(name, value)])?;
    expect_flag(
        &produce_flag(scene, StateUnit::Union, ".")?,
        "false",
        "malformed dynamic metadata is unavailable",
    )?;
    let manifest = fs::read_to_string(scene.runner.join("velnor/state-select/units"))?;
    let fields = manifest
        .lines()
        .next()
        .ok_or_else(|| fail("selector unit record is absent"))?
        .split(' ')
        .collect::<Vec<_>>();
    if fields.len() != 6 || !fields[empty_field].is_empty() {
        return Err(fail("malformed dynamic metadata remained available"));
    }
    Ok(())
}

fn malformed_records(record: &str) -> Result<Vec<(&'static str, String)>, Box<dyn Error>> {
    let (prefix, attested, produce) = record_fields(record)?;
    let other_root = typed_record(StateUnit::Union, "b")?;
    let (other_prefix, other_attested, other_produce) = record_fields(&other_root)?;
    let other_unit = typed_record(StateUnit::Suite, ".")?;
    let (_, suite_attested, suite_produce) = record_fields(&other_unit)?;
    let foreign_trust = StateScope {
        trust: StateTrust::Pr,
        unit: StateUnit::Union,
        root: unit_scope(StateUnit::Union, ".")?.root,
    }
    .prefix();
    let foreign_family = format!("{}trusted-union-root-", CacheFamily::Sources.key_prefix());
    Ok(vec![
        ("trailing LF", format!("{record}\n")),
        ("embedded LF", format!("{prefix}\n{attested} {produce}")),
        ("empty record", String::new()),
        ("invalid record", "bad record".to_owned()),
        (
            "invalid delimiter",
            format!("{prefix}; {attested} {produce}"),
        ),
        ("overlong record", "x".repeat(513)),
        (
            "foreign family",
            format!("{foreign_family} {attested} {produce}"),
        ),
        (
            "foreign trust",
            format!("{foreign_trust} {attested} {produce}"),
        ),
        (
            "cross-root prefix",
            format!("{other_prefix} {attested} {produce}"),
        ),
        (
            "cross-root attested output",
            format!("{prefix} {other_attested} {produce}"),
        ),
        (
            "cross-root produce output",
            format!("{prefix} {attested} {other_produce}"),
        ),
        (
            "foreign attested unit",
            format!("{prefix} {suite_attested} {produce}"),
        ),
        (
            "foreign produce unit",
            format!("{prefix} {attested} {suite_produce}"),
        ),
        (
            "swapped output suffixes",
            format!("{prefix} {produce} {attested}"),
        ),
    ])
}

#[test]
#[cfg(unix)]
fn selector_treats_trailing_newlines_in_dynamic_metadata_as_cold() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-newline")?;
    let units = [(StateUnit::Union, ".")];
    let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    gh.list(&rows_of(&[(T3.to_owned(), key.clone())]))?;
    gh.attest(&[Attestation::signed(&key)])?;
    run_select_units(&scene, &gh, &units, &[])?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&key),
        "valid dynamic metadata",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "false",
        "valid dynamic metadata",
    )?;

    let unmatched = unit_key(StateUnit::Union, ".", 0, 0x1001, 2)?;
    gh.list(&rows_of(&[(T3.to_owned(), unmatched.clone())]))?;
    gh.attest(&[Attestation::signed(&unmatched)])?;
    run_select_units(&scene, &gh, &units, &[])?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "true",
        "valid dynamic metadata without a matching candidate",
    )?;

    expect_unavailable_dynamic_metadata(
        &scene,
        &gh,
        "VELNOR_STATE_COMPAT_R0",
        &format!("{}\n", unit_compat(0)),
        4,
    )?;
    expect_unavailable_dynamic_metadata(
        &scene,
        &gh,
        "VELNOR_STATE_SNAPSHOT_R0",
        &format!("{}\n\n", unit_snapshot(0)),
        5,
    )?;

    run_select_units(
        &scene,
        &gh,
        &units,
        &[
            ("VELNOR_STATE_COMPAT_R0", ""),
            ("VELNOR_STATE_SNAPSHOT_R0", ""),
        ],
    )?;
    let manifest = fs::read_to_string(scene.runner.join("velnor/state-select/units"))?;
    if !manifest.lines().any(|line| line.ends_with("  ")) {
        return Err(fail(
            "empty dynamic metadata lost its six-field record shape",
        ));
    }
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "false",
        "empty dynamic metadata is unavailable",
    )?;

    let marker = scene.scratch.join("dynamic-value-executed");
    let injected = format!("{}; touch {}", unit_compat(0), marker.display());
    expect_unavailable_dynamic_metadata(&scene, &gh, "VELNOR_STATE_COMPAT_R0", &injected, 4)?;
    if marker.exists() {
        return Err(fail("dynamic metadata executed as shell syntax"));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn selector_rejects_malformed_static_unit_records_before_output() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-static-record")?;
    let record = typed_record(StateUnit::Union, ".")?;
    for (label, value) in malformed_records(&record)? {
        if run_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, ".")],
            &[("VELNOR_STATE_SELECT_UNIT_R0", value.as_str())],
        )
        .is_ok()
        {
            return Err(fail(&format!("selector accepted {label}")));
        }
        no_publication_or_network(&scene, &gh)?;
    }
    scene.finish()
}

fn expect_maximum_slug_associations(
    scene: &crate::gc1_scene::Scene,
    units: &[(StateUnit, &str)],
    selected: &[String],
) -> Result<(), Box<dyn Error>> {
    for (ordinal, (unit, root)) in units.iter().enumerate() {
        match ordinal {
            2 => {
                expect(
                    &selected_unit(scene, *unit, root)?,
                    std::slice::from_ref(&selected[0]),
                    "union output stays with its maximum-length root",
                )?;
                expect_flag(
                    &produce_flag(scene, *unit, root)?,
                    "false",
                    "union maximum-length root has a matching candidate",
                )?;
            }
            3 => {
                expect(
                    &selected_unit(scene, *unit, root)?,
                    std::slice::from_ref(&selected[1]),
                    "suite output stays with its maximum-length root",
                )?;
                expect_flag(
                    &produce_flag(scene, *unit, root)?,
                    "false",
                    "suite maximum-length root has a matching candidate",
                )?;
            }
            _ => {
                expect(
                    &selected_unit(scene, *unit, root)?,
                    &[],
                    "unmatched maximum-length root stays empty",
                )?;
                expect_flag(
                    &produce_flag(scene, *unit, root)?,
                    "true",
                    "unmatched maximum-length root requests production",
                )?;
            }
        }
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn selector_accepts_typed_records_for_65_maximum_slugs() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-maximum-slugs")?;
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let roots = (0..65)
        .map(|ordinal| {
            format!(
                "{}{}{}",
                "a".repeat(ROOT_SLUG_MAX_BYTES - 2),
                char::from(alphabet[ordinal / alphabet.len()]),
                char::from(alphabet[ordinal % alphabet.len()]),
            )
        })
        .collect::<Vec<_>>();
    let units = roots
        .iter()
        .enumerate()
        .map(|(ordinal, root)| {
            let unit = if ordinal % 2 == 0 {
                StateUnit::Union
            } else {
                StateUnit::Suite
            };
            (unit, root.as_str())
        })
        .collect::<Vec<_>>();
    let selected = [(2usize, StateUnit::Union), (3usize, StateUnit::Suite)]
        .into_iter()
        .map(|(ordinal, unit)| {
            unit_key(
                unit,
                units[ordinal].1,
                ordinal as u64,
                0x1000 + ordinal as u64,
                1,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rows = selected
        .iter()
        .map(|key| (T3.to_owned(), key.clone()))
        .collect::<Vec<_>>();
    gh.list(&rows_of(&rows))?;
    gh.attest(
        &selected
            .iter()
            .map(|key| Attestation::signed(key))
            .collect::<Vec<_>>(),
    )?;
    run_select_units(&scene, &gh, &units, &[])?;
    expect_maximum_slug_associations(&scene, &units, &selected)?;
    scene.finish()
}
