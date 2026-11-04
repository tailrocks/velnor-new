// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::RootSlug;
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_PENDING_BYTES_ENV, STATE_SEAL_PENDING_ENV, state_seal_job_id,
};
use velnor_actions_contract::{StepKind, StepRole};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{
    Attestation, Gh, MAIN, PULL, digest, key, run_select, scope, selected,
};
use crate::state_v4_seal::seal_jobs;

const ROOT: &str = ".";
const T1: &str = "2026-11-04T01:00:00Z";
const T2: &str = "2026-11-04T02:00:00Z";
const PRODUCE: &str = "state_union_root_produce";
const UNIT_BOUND: &str = "1616920640";

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
    scene.output_value(PRODUCE)
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
fn plan_reserves_only_the_later_eligible_roots() -> Result<(), Box<dyn Error>> {
    let jobs = seal_jobs(&["crates/velnor-runner", "."])?;
    let first = jobs
        .get(&state_seal_job_id(&RootSlug::from_root(
            "crates/velnor-runner",
        )?))
        .ok_or_else(|| fail("first seal job missing"))?;
    let second = jobs
        .get(&state_seal_job_id(&RootSlug::from_root(".")?))
        .ok_or_else(|| fail("second seal job missing"))?;
    let reservation =
        |job: &velnor_actions_contract::Job| -> Result<(String, String), Box<dyn Error>> {
            job.steps
                .iter()
                .find(|step| step.role == Some(StepRole::StateSealAdmit))
                .and_then(|step| match &step.kind {
                    StepKind::Shell { env, .. } => Some((
                        env.get(STATE_SEAL_PENDING_ENV)?.clone(),
                        env.get(STATE_SEAL_PENDING_BYTES_ENV)?.clone(),
                    )),
                    _ => None,
                })
                .ok_or_else(|| fail("admit step lost its reservation"))
        };
    let (pending, bytes) = reservation(first)?;
    if pending != "${{needs.plan.outputs.state_union_root_produce}}" || bytes != UNIT_BOUND {
        return Err(fail(&format!(
            "the first root must reserve the second: {pending} {bytes}"
        )));
    }
    let (pending, bytes) = reservation(second)?;
    if !pending.is_empty() || bytes != UNIT_BOUND {
        return Err(fail(&format!(
            "the last root reserves nothing: {pending} {bytes}"
        )));
    }
    Ok(())
}
