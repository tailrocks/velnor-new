// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{
    expect, expect_flag, produce_flag, rows_of, run_select_units, selected_unit, unit_compat,
    unit_key, unit_snapshot,
};

#[path = "state_v4_select_env_records.rs"]
mod records;
#[path = "state_v4_select_env_security.rs"]
mod security;

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

fn gh_path(gh: &Gh) -> Result<String, Box<dyn Error>> {
    Ok(std::path::Path::new(&gh.dir())
        .parent()
        .ok_or_else(|| fail("fake GH directory has no parent"))?
        .join("bin")
        .display()
        .to_string())
}

fn path_with_single_read_without_bash(scene: &Scene, gh: &Gh) -> Result<String, Box<dyn Error>> {
    let restricted =
        scene.restricted_path(&["sh", "printenv", "python3", "cat", "rm", "mkdir", "chmod"])?;
    let wrapper = fake_printenv(
        scene,
        "single-read-printenv",
        "case \"$1\" in VELNOR_STATE_COMPAT_R0) if [ -e \"$d/compat-read\" ]; then exit 1; fi; : > \"$d/compat-read\" ;; esac\ncommand -p printenv \"$@\"\n",
    )?;
    let wrapper_dir = wrapper
        .split(':')
        .next()
        .ok_or_else(|| fail("wrapped printenv path is missing"))?;
    Ok(format!("{wrapper_dir}:{restricted}:{}", gh_path(gh)?))
}

fn cold_manifest(scene: &Scene) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(
        fs::read_to_string(scene.runner.join("velnor/state-select/units"))?
            .lines()
            .next()
            .ok_or_else(|| fail("cold selector unit record is absent"))?
            .split(' ')
            .map(ToOwned::to_owned)
            .collect(),
    )
}

fn assert_cold_outputs(
    scene: &Scene,
    gh: &Gh,
    label: &str,
    expected_produce: &str,
    extra: &[(&str, &str)],
) -> Result<(), Box<dyn Error>> {
    run_select_units(scene, gh, &[(StateUnit::Union, ".")], extra)?;
    expect(&selected_unit(scene, StateUnit::Union, ".")?, &[], label)?;
    expect_flag(
        &produce_flag(scene, StateUnit::Union, ".")?,
        expected_produce,
        label,
    )
}

#[test]
#[cfg(unix)]
fn selector_requires_lossless_printenv_output_before_selection() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-printenv")?;
    let cases = [
        ("missing printenv", path_without_printenv(&scene)?),
        (
            "missing static record",
            fake_printenv(
                &scene,
                "missing-static-printenv",
                "case \"$1\" in VELNOR_STATE_SELECT_UNIT_R0) exit 1 ;; *) command -p printenv \"$@\" ;; esac\n",
            )?,
        ),
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
        "false",
        "failed dynamic lookup must be unavailable",
    )?;
    let manifest = fs::read_to_string(scene.runner.join("velnor/state-select/units"))?;
    let fields = manifest
        .lines()
        .next()
        .ok_or_else(|| fail("selector unit record is absent"))?
        .split(' ')
        .collect::<Vec<_>>();
    if fields.len() != 6 || !fields[4].is_empty() {
        return Err(fail("failed dynamic lookup remained available"));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn selector_colds_when_bash_or_repository_context_is_missing() -> Result<(), Box<dyn Error>> {
    let (scene, gh) = scene("seal-select-env-cold-inputs")?;
    let path = path_with_single_read_without_bash(&scene, &gh)?;
    assert_cold_outputs(
        &scene,
        &gh,
        "cold output uses the validated manifest",
        "true",
        &[("PATH", path.as_str())],
    )?;
    let fields = cold_manifest(&scene)?;
    let expected_compat = unit_compat(0);
    let expected_snapshot = unit_snapshot(0);
    if fields.len() != 6
        || fields[4].as_str() != expected_compat.as_str()
        || fields[5].as_str() != expected_snapshot.as_str()
    {
        return Err(fail(
            "cold selector did not use its validated metadata record",
        ));
    }

    assert_cold_outputs(
        &scene,
        &gh,
        "cold output preserves empty metadata fields",
        "false",
        &[
            ("PATH", path.as_str()),
            ("VELNOR_STATE_COMPAT_R0", ""),
            ("VELNOR_STATE_SNAPSHOT_R0", ""),
        ],
    )?;
    let fields = cold_manifest(&scene)?;
    if fields.len() != 6 || !fields[4].is_empty() || !fields[5].is_empty() {
        return Err(fail("cold selector lost empty trailing metadata fields"));
    }

    for (label, extra) in [
        ("missing ref", vec![("VELNOR_DEFAULT_REF", "")]),
        ("missing repository", vec![("GITHUB_REPOSITORY", "")]),
    ] {
        assert_cold_outputs(&scene, &gh, label, "true", &extra)?;
    }
    if std::path::Path::new(&gh.dir()).join("activity").exists() {
        return Err(fail("selector called gh during a cold fallback"));
    }
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
