// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::workflow::StepKind;
use velnor_actions_contract::workflow::cache_family::StateUnit;
use velnor_actions_contract::workflow::state_seal::{state_attested_output, state_produce_output};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{
    expect, expect_flag, produce_flag, rows_of, run_select_units, selected_unit, unit_compat,
    unit_key, unit_scope, unit_snapshot,
};

const T3: &str = "2026-11-04T03:00:00Z";

fn scene(name: &str) -> Result<(Scene, Gh), Box<dyn Error>> {
    let scene = Scene::new(name, crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    Ok((scene, gh))
}

fn no_publication_or_network(scene: &Scene, gh: &Gh) -> Result<(), Box<dyn Error>> {
    if scene.github_output.exists() || PathBuf::from(gh.dir()).join("activity").exists() {
        return Err(fail(
            "selector published output or called gh before validation",
        ));
    }
    Ok(())
}

fn fake_printenv(scene: &Scene, name: &str, body: &str) -> Result<String, Box<dyn Error>> {
    let dir = scene.scratch.join(name);
    fs::create_dir_all(&dir)?;
    let path = dir.join("printenv");
    fs::write(&path, format!("#!/bin/sh\n{body}"))?;
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions)?;
    Ok(format!(
        "{}:{}:{}",
        dir.display(),
        scene.scratch.join("bin").display(),
        std::env::var("PATH")?
    ))
}

fn path_without_printenv(scene: &Scene) -> Result<String, Box<dyn Error>> {
    let dir = scene.scratch.join("path-without-printenv");
    fs::create_dir_all(&dir)?;
    std::os::unix::fs::symlink("/bin/sh", dir.join("sh"))?;
    Ok(dir.to_string_lossy().into_owned())
}

#[test]
#[cfg(unix)]
fn selector_treats_trailing_newlines_in_dynamic_metadata_as_cold() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-newline")?;
    let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    gh.list(&rows_of(&[(T3.to_owned(), key.clone())]))?;
    gh.attest(&[Attestation::signed(&key)])?;
    let units = [(StateUnit::Union, ".")];

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

    let compat = unit_compat(0);
    let snapshot = unit_snapshot(0);
    for (name, value) in [
        ("VELNOR_STATE_COMPAT_R0", format!("{compat}\n")),
        ("VELNOR_STATE_SNAPSHOT_R0", format!("{snapshot}\n\n")),
    ] {
        run_select_units(&scene, &gh, &units, &[(name, value.as_str())])?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, ".")?,
            "true",
            "malformed dynamic metadata must force cold production",
        )?;
    }

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

    let marker = scene.scratch.join("dynamic-value-executed");
    let injected = format!("{compat}; touch {}", marker.display());
    run_select_units(
        &scene,
        &gh,
        &units,
        &[("VELNOR_STATE_COMPAT_R0", injected.as_str())],
    )?;
    if marker.exists() {
        return Err(fail("dynamic metadata executed as shell syntax"));
    }
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "true",
        "malformed dynamic metadata cannot produce a warm hit",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn selector_rejects_malformed_static_unit_records_before_output() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-static-record")?;
    let step = state_select_step(
        &[SealRoot {
            unit: StateUnit::Union,
            slug: unit_scope(StateUnit::Union, ".")?.root,
        }],
        JobTimeout::PLAN,
    )?;
    let StepKind::Shell { env, .. } = step.kind else {
        return Err(fail("selector step is not a shell step"));
    };
    let record = env
        .get("VELNOR_STATE_SELECT_UNIT_R0")
        .ok_or_else(|| fail("selector static unit record is absent"))?;
    let overlong = "x".repeat(513);
    let malformed = [
        format!("{record}\n"),
        String::new(),
        "bad record".to_owned(),
        overlong,
    ];
    for value in malformed {
        if run_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, ".")],
            &[("VELNOR_STATE_SELECT_UNIT_R0", value.as_str())],
        )
        .is_ok()
        {
            return Err(fail("selector accepted a malformed static unit record"));
        }
        no_publication_or_network(&scene, &gh)?;
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn selector_requires_lossless_printenv_output_before_selection() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-printenv")?;
    let cases = [
        ("missing printenv", path_without_printenv(&scene)?),
        (
            "partial failed static printenv",
            fake_printenv(&scene, "partial-printenv", "printf partial; exit 7\n")?,
        ),
        (
            "successful printenv without line terminator",
            fake_printenv(
                &scene,
                "unterminated-printenv",
                "case \"$1\" in VELNOR_STATE_SELECT_UNIT_R0) printf 'bad' ;; *) command -p printenv \"$@\" ;; esac\n",
            )?,
        ),
    ];
    for (label, path) in cases {
        let result = run_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, ".")],
            &[("PATH", path.as_str())],
        );
        if result.is_ok() {
            return Err(fail(&format!("selector accepted {label}")));
        }
        no_publication_or_network(&scene, &gh)?;
    }

    let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    gh.list(&rows_of(&[(T3.to_owned(), key.clone())]))?;
    gh.attest(&[Attestation::signed(&key)])?;
    let dynamic_path = fake_printenv(
        &scene,
        "failed-dynamic-printenv",
        "case \"$1\" in VELNOR_STATE_COMPAT_R0) printf partial; exit 7 ;; *) command -p printenv \"$@\" ;; esac\n",
    )?;
    run_select_units(
        &scene,
        &gh,
        &[(StateUnit::Union, ".")],
        &[("PATH", dynamic_path.as_str())],
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "true",
        "failed dynamic lookup must be unavailable",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn selector_discards_partial_printenv_output_on_static_lookup_failure() -> Result<(), Box<dyn Error>>
{
    let (scene, gh) = scene("seal-select-env-partial")?;
    let path = fake_printenv(&scene, "partial-unit-printenv", "printf partial; exit 7\n")?;
    if run_select_units(
        &scene,
        &gh,
        &[(StateUnit::Union, ".")],
        &[("PATH", path.as_str())],
    )
    .is_ok()
    {
        return Err(fail("selector accepted partial printenv output"));
    }
    no_publication_or_network(&scene, &gh)?;
    scene.finish()
}
