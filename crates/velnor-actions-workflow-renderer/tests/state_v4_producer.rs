// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::cache_family::{
    RootSlug, SliceId, StateScope, StateTrust, StateUnit,
};
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_MATRIX_OUTPUT;
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealUnit, state_select_step};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{
    Attestation, Gh, MAIN, OTHER_WORKFLOW_REF, PULL, REPO, digest, key, run_select, scope,
    script_of, selected,
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

type Legs = Vec<(String, String)>;

fn exact_keys(hits: &[&str], compat: &str, snapshot: &str) -> Result<Vec<String>, Box<dyn Error>> {
    hits.iter()
        .map(|slice| {
            let scope = StateScope {
                trust: StateTrust::Trusted,
                unit: StateUnit::Slice(SliceId::parse(slice)?),
            };
            Ok(format!(
                "{}{compat}-{snapshot}-{}",
                scope.prefix(),
                digest('9')
            ))
        })
        .collect()
}

fn matrix_legs(scene: &Scene, output: &str) -> Result<Legs, Box<dyn Error>> {
    let text = scene.output_value(output)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let matrix: serde_json::Value = serde_json::from_str(&text)?;
    Ok(matrix["include"]
        .as_array()
        .ok_or_else(|| fail(&format!("{output} has no include list")))?
        .iter()
        .map(|leg| {
            (
                leg["slice"].as_str().unwrap_or_default().to_owned(),
                leg["pending"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect())
}

fn select_legs(
    scene: &Scene,
    gh: &Gh,
    units: &[(&str, &str)],
    hits: &[&str],
) -> Result<BTreeMap<String, Legs>, Box<dyn Error>> {
    let (compat, snapshot) = (digest('a'), digest('1'));
    let units = units
        .iter()
        .map(|(slice, root)| {
            Ok(SealUnit {
                slice: SliceId::parse(slice)?,
                root: RootSlug::from_root(root)?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let exact = exact_keys(hits, &compat, &snapshot)?;
    let listed: Vec<(&str, &str, &str)> =
        exact.iter().map(|key| (T2, MAIN, key.as_str())).collect();
    gh.list(&listed)?;
    let signed: Vec<Attestation> = exact.iter().map(|key| Attestation::signed(key)).collect();
    gh.attest(&signed)?;
    let script = script_of(&state_select_step(&units)?)?;
    let dir = gh.dir();
    let mut env = vec![
        ("FAKE_GH_DIR".to_owned(), dir.clone()),
        ("GITHUB_REPOSITORY".to_owned(), REPO.to_owned()),
        (
            "GITHUB_WORKFLOW_REF".to_owned(),
            OTHER_WORKFLOW_REF.to_owned(),
        ),
        ("GH_TOKEN".to_owned(), "fixture".to_owned()),
    ];
    for ordinal in 0..units.len() {
        env.push((format!("VELNOR_STATE_COMPAT_R{ordinal}"), compat.clone()));
        env.push((
            format!("VELNOR_STATE_SNAPSHOT_R{ordinal}"),
            snapshot.clone(),
        ));
    }
    let pairs: Vec<(&str, &str)> = env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    scene.run_script(&script, &pairs, None)?;
    [
        "state_matrix_seal",
        "state_matrix_producer_root",
        "state_matrix_producer_crates_velnor_runner",
    ]
    .into_iter()
    .map(|output| Ok((output.to_owned(), matrix_legs(scene, output)?)))
    .collect()
}

fn legs(names: &[(&str, &str)]) -> Legs {
    names
        .iter()
        .map(|(slice, pending)| ((*slice).to_owned(), (*pending).to_owned()))
        .collect()
}

#[test]
#[cfg(unix)]
fn plan_reserves_the_largest_later_bound_for_each_leg() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("producer-reserve", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let matrices = select_legs(
        &scene,
        &gh,
        &[
            ("runner", "crates/velnor-runner"),
            ("first", "."),
            ("hit", "."),
        ],
        &["hit"],
    )?;
    assert_eq!(
        matrices["state_matrix_seal"],
        legs(&[("runner", UNIT_BOUND), ("first", "0")]),
        "a leg reserves the largest bound of the legs after it, and an exact hit is no leg"
    );
    scene.finish()
}

#[test]
#[cfg(unix)]
fn the_matrices_list_only_the_slices_that_must_be_produced() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("producer-matrices", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let matrices = select_legs(
        &scene,
        &gh,
        &[
            ("a", "."),
            ("b", "."),
            ("c", "crates/velnor-runner"),
            ("d", "crates/velnor-runner"),
        ],
        &["b", "c"],
    )?;
    assert_eq!(
        matrices["state_matrix_seal"],
        legs(&[("a", UNIT_BOUND), ("d", "0")]),
        "the seal matrix"
    );
    assert_eq!(
        matrices["state_matrix_producer_root"],
        legs(&[("a", "")]),
        "the root producer matrix"
    );
    assert_eq!(
        matrices["state_matrix_producer_crates_velnor_runner"],
        legs(&[("d", "")]),
        "the runner producer matrix"
    );
    scene.finish()
}
