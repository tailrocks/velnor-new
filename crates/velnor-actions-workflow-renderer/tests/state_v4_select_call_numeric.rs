// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{
    expect, expect_flag, produce_flag, run_select_units, selected_unit, unit_key,
};

#[test]
fn invalid_listing_call_budgets_start_no_gh_process() -> Result<(), Box<dyn Error>> {
    for value in ["invalid", "NaN", "Infinity", "0", "-1", "1e-9999", "3600.1"] {
        eprintln!("X101 case value={value}");
        let scene = Scene::new(
            "seal-select-invalid-call-budget",
            crate::gc1_t53::scripts()?,
        )?;
        let gh = Gh::install(&scene)?;
        let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        gh.list(&crate::gc1_seal_units::rows_of(&[(
            crate::gc1_seal_units::at(0),
            key.clone(),
        )]))?;
        gh.attest(&[Attestation::signed(&key)])?;
        let invocations = PathBuf::from(gh.dir()).join("invocations");
        fs::write(&invocations, "")?;
        run_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, ".")],
            &[("VELNOR_STATE_GH_TIMEOUT", value)],
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &[],
            "invalid GH call budget",
        )?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, ".")?,
            "true",
            "invalid GH call budget",
        )?;
        if !fs::read_to_string(&invocations)?.is_empty() {
            return Err(fail("invalid GH call budget started a GH process"));
        }
        scene.finish()?;
    }
    Ok(())
}

#[test]
fn valid_listing_call_budgets_keep_signed_candidates() -> Result<(), Box<dyn Error>> {
    for value in [
        None,
        Some("10.5"),
        Some("1_0"),
        Some("120"),
        Some("300"),
        Some("3600"),
    ] {
        match value {
            None => eprintln!("X101 case value=default"),
            Some(value) => eprintln!("X101 case value={value}"),
        }
        let scene = Scene::new("seal-select-valid-call-budget", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        gh.list(&crate::gc1_seal_units::rows_of(&[(
            crate::gc1_seal_units::at(0),
            key.clone(),
        )]))?;
        gh.attest(&[Attestation::signed(&key)])?;

        let extra = value
            .map(|limit| vec![("VELNOR_STATE_GH_TIMEOUT", limit)])
            .unwrap_or_default();
        run_select_units(&scene, &gh, &[(StateUnit::Union, ".")], &extra)?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            std::slice::from_ref(&key),
            "valid GH call budget",
        )?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, ".")?,
            "false",
            "valid GH call budget",
        )?;
        scene.finish()?;
    }
    Ok(())
}
