// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{
    expect_flag, produce_flag, rows_of, run_select_units, selected_unit, unit_compat, unit_key,
};

use super::{T3, scene};

#[test]
#[cfg(unix)]
fn selector_reader_files_have_private_modes() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-private-reader")?;
    run_select_units(&scene, &gh, &[(StateUnit::Union, ".")], &[])?;
    let directory = scene.runner.join("velnor/state-select");
    let value = directory.join("value");
    let directory_mode = fs::metadata(&directory)?.permissions().mode() & 0o777;
    let value_mode = fs::metadata(&value)?.permissions().mode() & 0o777;
    if directory_mode != 0o700 || value_mode != 0o600 {
        return Err(fail(&format!(
            "selector reader permissions were {directory_mode:o}/{value_mode:o}"
        )));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn selector_treats_missing_and_extra_dynamic_reader_bytes_as_unavailable()
-> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-reader-dynamic-boundary")?;
    let extra = format!(
        "case \"$1\" in VELNOR_STATE_COMPAT_R0) printf '%s\\nextra' '{}' ;; *) command -p printenv \"$@\" ;; esac\n",
        unit_compat(0)
    );
    let cases = [
        (
            "missing dynamic value",
            super::fake_printenv(
                &scene,
                "missing-dynamic-printenv",
                "case \"$1\" in VELNOR_STATE_COMPAT_R0) exit 1 ;; *) command -p printenv \"$@\" ;; esac\n",
            )?,
        ),
        (
            "unterminated extra bytes",
            super::fake_printenv(&scene, "extra-dynamic-printenv", &extra)?,
        ),
    ];
    for (label, path) in cases {
        expect_dynamic_cold(&scene, &gh, label, &[("PATH", path.as_str())])?;
    }
    let embedded = format!("{}\nextra", unit_compat(0));
    expect_dynamic_cold(
        &scene,
        &gh,
        "embedded LF",
        &[("VELNOR_STATE_COMPAT_R0", embedded.as_str())],
    )?;
    scene.finish()
}

fn expect_dynamic_cold(
    scene: &Scene,
    gh: &Gh,
    label: &str,
    extra: &[(&str, &str)],
) -> Result<(), Box<dyn Error>> {
    run_select_units(scene, gh, &[(StateUnit::Union, ".")], extra)?;
    expect_flag(&produce_flag(scene, StateUnit::Union, ".")?, "false", label)?;
    let manifest = fs::read_to_string(scene.runner.join("velnor/state-select/units"))?;
    let fields = manifest
        .lines()
        .next()
        .ok_or_else(|| fail("selector unit record is absent"))?
        .split(' ')
        .collect::<Vec<_>>();
    if fields.len() != 6 || !fields[4].is_empty() {
        return Err(fail(&format!("{label} remained available in the manifest")));
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn selector_ignores_inherited_ordinals_and_frame_values() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-inherited-ordinals")?;
    let units = [(StateUnit::Union, "."), (StateUnit::Suite, "a")];
    let candidate = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    gh.list(&rows_of(&[(T3.to_owned(), candidate.clone())]))?;
    gh.attest(&[Attestation::signed(&candidate)])?;
    run_select_units(
        &scene,
        &gh,
        &units,
        &[
            ("ordinals", "1 0 64"),
            ("i", "64"),
            ("state_prefix", "foreign-"),
            ("seen", "poison"),
            ("line", "poison"),
            ("record_name", "poison"),
            ("record", "poison"),
            ("prefix", "poison"),
            ("rest", "poison"),
            ("attested", "poison"),
            ("produce", "poison"),
            ("unit_name", "poison"),
            ("root", "poison"),
            ("slug", "poison"),
            ("remaining", "poison"),
            ("character", "poison"),
            ("compat_name", "poison"),
            ("snapshot_name", "poison"),
            ("compat", "poison"),
            ("snapshot", "poison"),
            ("result", "poison"),
            ("extra", "poison"),
            ("ordinal", "poison"),
            ("VELNOR_STATE_SELECT_UNIT_R65", "poison"),
            ("VELNOR_STATE_COMPAT_R65", ""),
            ("VELNOR_STATE_SNAPSHOT_R65", "poison"),
            ("SPOOF", "value\nrecord=poison"),
            ("VELNOR_STATE_SELECT_TAG", "attested"),
            ("VELNOR_STATE_SELECT_KIND", "foreign"),
            ("VELNOR_STATE_SELECT_UNIT_PREFIX", "foreign-"),
            ("VELNOR_STATE_SELECT_UNIT", "r64"),
        ],
    )?;
    let manifest = fs::read_to_string(scene.runner.join("velnor/state-select/units"))?;
    let records = manifest.lines().collect::<Vec<_>>();
    if records.len() != units.len()
        || !records[0].starts_with("velnor-v4-state-trusted-union-")
        || !records[0].contains(" r0 ")
        || !records[1].starts_with("velnor-v4-state-trusted-suite-")
        || !records[1].contains(" r1 ")
    {
        return Err(fail(
            "inherited ordinals changed the validated unit manifest",
        ));
    }
    for (unit, root) in units {
        let expected = if unit == StateUnit::Union {
            vec![candidate.clone()]
        } else {
            Vec::new()
        };
        if selected_unit(&scene, unit, root)? != expected {
            return Err(fail("inherited ordinals changed selected outputs"));
        }
        expect_flag(
            &produce_flag(&scene, unit, root)?,
            if unit == StateUnit::Union {
                "false"
            } else {
                "true"
            },
            "inherited ordinal isolation",
        )?;
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn selector_treats_lossy_compat_metadata_as_unavailable() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-lossy-compat")?;
    let candidate = unit_key(StateUnit::Union, ".", 0, 0x1001, 3)?;
    gh.list(&rows_of(&[(T3.to_owned(), candidate.clone())]))?;
    gh.attest(&[Attestation::signed(&candidate)])?;
    let compat = format!("{}\n", unit_compat(0));
    run_select_units(
        &scene,
        &gh,
        &[(StateUnit::Union, ".")],
        &[("VELNOR_STATE_COMPAT_R0", compat.as_str())],
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "false",
        "trailing LF makes mismatched snapshot metadata unavailable",
    )?;
    scene.finish()
}
