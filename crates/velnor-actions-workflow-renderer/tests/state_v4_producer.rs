// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::RootSlug;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_MATRIX_OUTPUT;
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealUnit, state_select_step};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{
    Attestation, Gh, MAIN, PULL, REPO, digest, key, run_select, scope, script_of, selected,
    slice_of,
};

const ROOT: &str = ".";
const T1: &str = "2026-11-04T01:00:00Z";
const T2: &str = "2026-11-04T02:00:00Z";
const UNIT_BOUND: &str = "808460320";

fn produce_with(
    scene: &Scene,
    gh: &Gh,
    compat: &str,
    snapshot: &str,
) -> Result<String, Box<dyn Error>> {
    run_select(
        scene,
        gh,
        ROOT,
        &[
            ("VELNOR_STATE_COMPAT_R0", compat),
            ("VELNOR_STATE_SNAPSHOT_R0", snapshot),
        ],
    )?;
    Ok((!scene.output_value(STATE_SEAL_MATRIX_OUTPUT)?.is_empty()).to_string())
}

fn expect(got: &str, want: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if got == want {
        Ok(())
    } else {
        Err(fail(&format!("{label}: produce={got}, wanted {want}")))
    }
}

#[test]
#[cfg(unix)]
fn eligibility_follows_the_live_attested_listing() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("producer-eligible", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let compat = digest('a');
    let snapshot = digest('1');
    let exact = key('a', '1', '1')?;
    let predecessor = key('a', '2', '2')?;
    let forged = key('a', '1', '9')?;
    gh.list(&[])?;
    expect(
        &produce_with(&scene, &gh, &compat, &snapshot)?,
        "true",
        "no entry",
    )?;

    gh.list(&[(T2, MAIN, &exact)])?;
    gh.attest(&[Attestation::signed(&exact)])?;
    expect(
        &produce_with(&scene, &gh, &compat, &snapshot)?,
        "false",
        "attested exact entry",
    )?;

    gh.list(&[])?;
    expect(
        &produce_with(&scene, &gh, &compat, &snapshot)?,
        "true",
        "the exact entry was evicted",
    )?;

    gh.list(&[(T2, MAIN, &exact)])?;
    gh.attest(&[])?;
    expect(
        &produce_with(&scene, &gh, &compat, &snapshot)?,
        "true",
        "saved but unattested",
    )?;

    gh.list(&[(T2, MAIN, &forged), (T1, MAIN, &predecessor)])?;
    gh.attest(&[Attestation::signed(&predecessor)])?;
    expect(
        &produce_with(&scene, &gh, &compat, &snapshot)?,
        "true",
        "forged entry at the exact prefix beside an attested predecessor",
    )?;
    if selected(&scene, ROOT)? != [predecessor.clone()] {
        return Err(fail(
            "the predecessor must stay selectable for a warm start",
        ));
    }

    scene.finish()
}

#[test]
#[cfg(unix)]
fn eligibility_ignores_entries_of_other_identities_and_unusable_inputs()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("producer-foreign", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let compat = digest('a');
    let snapshot = digest('1');
    let exact = key('a', '1', '1')?;
    let other_compat = key('b', '1', '3')?;
    let foreign_root = format!(
        "{}{}-{}-{}",
        scope()?.prefix().replacen("-root-", "-other-", 1),
        digest('a'),
        digest('1'),
        digest('4')
    );
    gh.list(&[(T2, MAIN, &other_compat)])?;
    gh.attest(&[Attestation::signed(&other_compat)])?;
    expect(
        &produce_with(&scene, &gh, &compat, &snapshot)?,
        "true",
        "attested exact snapshot under another compat",
    )?;

    gh.list(&[(T2, MAIN, &foreign_root)])?;
    gh.attest(&[Attestation::signed(&foreign_root)])?;
    expect(
        &produce_with(&scene, &gh, &compat, &snapshot)?,
        "true",
        "another root's attested exact entry",
    )?;

    gh.list(&[(T2, MAIN, &exact)])?;
    gh.attest(&[Attestation::signed(&exact)])?;
    expect(
        &produce_with(&scene, &gh, "", &snapshot)?,
        "false",
        "no compat computed",
    )?;
    expect(
        &produce_with(&scene, &gh, &compat, "")?,
        "false",
        "no snapshot computed",
    )?;

    gh.list(&[(T2, PULL, &exact)])?;
    expect(
        &produce_with(&scene, &gh, &compat, &snapshot)?,
        "true",
        "an entry on a pull request ref",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn plan_reserves_only_the_later_eligible_roots() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("producer-reserve", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    gh.list(&[])?;
    let units = ["crates/velnor-runner", "."]
        .iter()
        .map(|root| {
            Ok(SealUnit {
                slice: slice_of(root)?,
                root: RootSlug::from_root(root)?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let script = script_of(&state_select_step(&units)?)?;
    let (compat, snapshot) = (digest('a'), digest('1'));
    let dir = gh.dir();
    scene.run_script(
        &script,
        &[
            ("FAKE_GH_DIR", dir.as_str()),
            ("GITHUB_REPOSITORY", REPO),
            ("GH_TOKEN", "fixture"),
            ("VELNOR_STATE_COMPAT_R0", compat.as_str()),
            ("VELNOR_STATE_SNAPSHOT_R0", snapshot.as_str()),
            ("VELNOR_STATE_COMPAT_R1", compat.as_str()),
            ("VELNOR_STATE_SNAPSHOT_R1", snapshot.as_str()),
        ],
        None,
    )?;
    let matrix: serde_json::Value =
        serde_json::from_str(&scene.output_value(STATE_SEAL_MATRIX_OUTPUT)?)?;
    let pending: Vec<(String, String)> = matrix["include"]
        .as_array()
        .ok_or_else(|| fail("the seal matrix has no include list"))?
        .iter()
        .map(|leg| {
            (
                leg["slice"].as_str().unwrap_or_default().to_owned(),
                leg["pending"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    if pending
        != [
            ("crates-velnor-runner".to_owned(), UNIT_BOUND.to_owned()),
            ("root".to_owned(), "0".to_owned()),
        ]
    {
        return Err(fail(&format!(
            "the first root must reserve the second and the last nothing: {pending:?}"
        )));
    }
    scene.finish()
}
