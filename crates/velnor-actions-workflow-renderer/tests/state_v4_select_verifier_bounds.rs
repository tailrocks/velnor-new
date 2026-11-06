// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::Scene;
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{
    expect, expect_flag, hang_keys, produce_flag, run_select_units, selected_unit, unit_key,
};

#[test]
fn reserved_verifier_failures_clear_the_whole_unit() -> Result<(), Box<dyn Error>> {
    for status in [124, 125, 126, 127, 137] {
        eprintln!("X101 case status={status}");
        let scene = Scene::new("seal-select-verifier-status", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let accepted = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        let failed = unit_key(StateUnit::Union, ".", 0, 0x1000, 2)?;
        gh.list(&[
            (
                "2026-11-04T03:00:00Z",
                crate::gc1_seal_support::MAIN,
                &accepted,
            ),
            (
                "2026-11-04T02:00:00Z",
                crate::gc1_seal_support::MAIN,
                &failed,
            ),
        ])?;
        gh.attest(&[Attestation::signed(&accepted), Attestation::signed(&failed)])?;
        fs::write(
            PathBuf::from(gh.dir()).join("fail-statuses"),
            format!("{failed} {status}\n"),
        )?;
        run_select_units(&scene, &gh, &[(StateUnit::Union, ".")], &[])?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &[],
            "reserved verifier failure",
        )?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, ".")?,
            "true",
            "reserved verifier failure",
        )?;
        scene.finish()?;
    }
    Ok(())
}

#[test]
fn ordinary_verifier_rejection_allows_later_candidate() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-verifier-rejection", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let rejected = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let accepted = unit_key(StateUnit::Union, ".", 0, 0x1000, 2)?;
    gh.list(&[
        (
            "2026-11-04T03:00:00Z",
            crate::gc1_seal_support::MAIN,
            &rejected,
        ),
        (
            "2026-11-04T02:00:00Z",
            crate::gc1_seal_support::MAIN,
            &accepted,
        ),
    ])?;
    gh.attest(&[
        Attestation::signed(&rejected),
        Attestation::signed(&accepted),
    ])?;
    fs::write(
        PathBuf::from(gh.dir()).join("fail-statuses"),
        format!("{rejected} 1\n"),
    )?;
    run_select_units(&scene, &gh, &[(StateUnit::Union, ".")], &[])?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&accepted),
        "ordinary verifier rejection",
    )?;
    scene.finish()
}

#[path = "state_v4_select_verifier_resistant.rs"]
mod resistant;

#[test]
fn resistant_verifier_timeout_clears_its_unit_and_keeps_sibling() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new(
        "seal-select-call-timeout-cleanup",
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let roots = [".", "b"];
    let units = roots.map(|root| (StateUnit::Union, root));
    let earlier = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let hung = unit_key(StateUnit::Union, ".", 0, 0x1000, 2)?;
    let sibling = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
    gh.list(&[
        ("2026-11-04T04:00:00Z", MAIN, &earlier),
        ("2026-11-04T03:00:00Z", MAIN, &hung),
        ("2026-11-04T02:00:00Z", MAIN, &sibling),
    ])?;
    gh.attest(&[
        Attestation::signed(&earlier),
        Attestation::signed(&hung),
        Attestation::signed(&sibling),
    ])?;
    hang_keys(&gh, &[&hung])?;
    run_select_units(
        &scene,
        &gh,
        &units,
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "2"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
            ("VELNOR_STATE_SELECT_DEADLINE", "30"),
        ],
    )?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        &[],
        "unit with a resistant verifier",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "true",
        "unit with a resistant verifier",
    )?;
    expect(
        &selected_unit(&scene, StateUnit::Union, "b")?,
        std::slice::from_ref(&sibling),
        "healthy sibling during verifier timeout",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, "b")?,
        "false",
        "healthy sibling during verifier timeout",
    )?;
    crate::gc1_seal_units::hung_processes_gone(&gh)?;
    scene.finish()
}
