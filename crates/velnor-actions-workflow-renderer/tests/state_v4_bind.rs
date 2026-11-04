// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::S256;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_SUBJECT_REL;
use velnor_actions_contract::{Job, StepKind, StepRole};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, ENTRY_ID, ENTRY_SIZE, Gh, MAIN, PULL, REPO};
use crate::state_v4_admit::Admit;
use crate::state_v4_attest_save::BOUND;
use crate::state_v4_seal::{FIXTURE, identity_expression, resolved_prefix, seal_job, step_of};

pub(crate) fn run_bind(scene: &Scene, gh: &Gh, job: &Job, key: &str) -> Result<(), Box<dyn Error>> {
    let step = step_of(job, StepRole::StateSealBind)?;
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(fail("the bind step is not a shell step"));
    };
    match std::fs::remove_file(&scene.github_output) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    scene.install_shim("sleep", crate::gc1_seal_support::SLEEP_SHIM)?;
    let dir = gh.dir();
    let env = [
        ("FAKE_GH_DIR", dir.as_str()),
        ("GITHUB_REF", MAIN),
        ("VELNOR_STATE_SEAL_KEY", key),
        ("GITHUB_REPOSITORY", REPO),
    ];
    scene.run_script(run.get(2).ok_or_else(|| fail("no script"))?, &env, None)?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn bind_attests_the_saved_entry_by_key_id_and_size() -> Result<(), Box<dyn Error>> {
    let job = seal_job()?;
    let scene = Scene::new("seal-bind", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let subject = scene.runner.join(STATE_SEAL_SUBJECT_REL);
    let prefix = resolved_prefix(&identity_expression()?);
    let sealed = format!("{prefix}{}", S256::of(FIXTURE));
    let planted = format!("{sealed}-{}", S256::of(b"planted"));
    let at = "2026-11-04T01:00:00Z";
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    std::fs::write(scene.payload(), FIXTURE)?;

    gh.list_entries(&[
        (at, MAIN, &planted, ENTRY_ID + 1, ENTRY_SIZE + 1),
        (at, MAIN, &sealed, ENTRY_ID, ENTRY_SIZE),
    ])?;
    run_bind(&scene, &gh, &job, &sealed)?;
    let want = format!("{sealed} {ENTRY_ID} {ENTRY_SIZE}\n");
    if scene.output_value("bound")? != "true" || std::fs::read(&subject)? != want.as_bytes() {
        return Err(fail(
            "the subject is not the exact key, its cache id and its size",
        ));
    }

    gh.list_entries(&[(at, MAIN, &planted, ENTRY_ID + 1, ENTRY_SIZE)])?;
    run_bind(&scene, &gh, &job, &sealed)?;
    if scene.output_value("bound")? != "false" || subject.exists() {
        return Err(fail("a key-prefix neighbour was bound in place of the key"));
    }

    gh.list_entries(&[
        (at, MAIN, &sealed, ENTRY_ID, ENTRY_SIZE),
        (at, MAIN, &sealed, ENTRY_ID + 2, ENTRY_SIZE),
    ])?;
    run_bind(&scene, &gh, &job, &sealed)?;
    if scene.output_value("bound")? != "false" || subject.exists() {
        return Err(fail("an ambiguous key was bound"));
    }

    gh.list_entries(&[])?;
    run_bind(&scene, &gh, &job, &sealed)?;
    if scene.output_value("bound")? != "false" || subject.exists() {
        return Err(fail("a missing entry was bound"));
    }

    gh.list_entries(&[(at, MAIN, &sealed, ENTRY_ID, BOUND)])?;
    run_bind(&scene, &gh, &job, &sealed)?;
    if scene.output_value("bound")? != "true" || !subject.exists() {
        return Err(fail("an entry exactly at the outer bound was refused"));
    }

    gh.list_entries(&[(at, MAIN, &sealed, ENTRY_ID, BOUND + 1)])?;
    run_bind(&scene, &gh, &job, &sealed)?;
    if scene.output_value("bound")? != "false" || subject.exists() {
        return Err(fail("an entry above the outer bound was bound"));
    }

    let forged = format!("{sealed} 99 1");
    gh.list_entries(&[(at, MAIN, &forged, ENTRY_ID, ENTRY_SIZE)])?;
    run_bind(&scene, &gh, &job, &sealed)?;
    if scene.output_value("bound")? != "false" || subject.exists() {
        return Err(fail(
            "a key with a forged id and size after a space was bound",
        ));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn an_existing_entry_ends_the_run_and_only_an_unattested_one_is_deleted()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-preexisting", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let admit = Admit::new()?;
    let prefix = resolved_prefix(&identity_expression()?);
    let sealed = format!("{prefix}{}", S256::of(FIXTURE));
    let neighbour = format!("{sealed}-{}", S256::of(b"neighbour"));
    let forged = format!("{sealed} 99 1");
    let at = "2026-11-04T01:00:00Z";
    let size = u64::try_from(FIXTURE.len())?;
    let admitted = |gh: &Gh| admit.run_keyed(&scene, gh, Some(size), "", 0, &sealed);

    gh.list_entries(&[])?;
    if !admitted(&gh)?.0 {
        return Err(fail("a key with no entry was not admitted"));
    }

    gh.list_entries(&[
        (at, MAIN, &neighbour, ENTRY_ID + 1, ENTRY_SIZE),
        (at, MAIN, &forged, ENTRY_ID + 2, ENTRY_SIZE),
        (at, PULL, &sealed, ENTRY_ID + 3, ENTRY_SIZE),
    ])?;
    if !admitted(&gh)?.0 {
        return Err(fail("entries of other keys or other refs blocked the save"));
    }

    gh.list_entries(&[
        (at, MAIN, &sealed, ENTRY_ID, ENTRY_SIZE),
        (at, MAIN, &neighbour, ENTRY_ID + 1, ENTRY_SIZE),
    ])?;
    gh.attest(&[Attestation::bound(&sealed, ENTRY_ID, ENTRY_SIZE)])?;
    let (ok, output) = admitted(&gh)?;
    if ok || !output.contains("already_attested") || !gh.log().is_empty() {
        return Err(fail(&format!(
            "an attested entry must end the run without a delete: {output} {}",
            gh.log()
        )));
    }
    if gh.rows()?.len() != 2 {
        return Err(fail("an attested entry was removed"));
    }

    gh.attest(&[])?;
    let (ok, output) = admitted(&gh)?;
    let remaining: Vec<String> = gh.rows()?.into_iter().map(|(key, _)| key).collect();
    if ok
        || !output.contains("preexisting_unattested_deleted")
        || gh.log() != format!("delete cache {ENTRY_ID}\n")
        || remaining != [neighbour.clone()]
    {
        return Err(fail(&format!(
            "an unattested entry must be deleted by its exact id and refuse the run: {output} {} {remaining:?}",
            gh.log()
        )));
    }
    gh.clear_log()?;

    gh.list_entries(&[(at, MAIN, &sealed, ENTRY_ID, ENTRY_SIZE)])?;
    gh.refuse_delete(&[ENTRY_ID])?;
    let (ok, output) = admitted(&gh)?;
    if ok || !output.contains("preexisting_unattested_undeletable") {
        return Err(fail(&format!(
            "an entry that cannot be deleted was admitted: {output}"
        )));
    }
    gh.refuse_delete(&[])?;

    gh.mode("fail")?;
    let (ok, output) = admitted(&gh)?;
    if ok || !output.contains("existing_unlistable") {
        return Err(fail(&format!("an unlistable cache was admitted: {output}")));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn bind_waits_for_a_listing_that_lags_the_save() -> Result<(), Box<dyn Error>> {
    let job = seal_job()?;
    let scene = Scene::new("seal-bind-lag", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let subject = scene.runner.join(STATE_SEAL_SUBJECT_REL);
    let sealed = format!(
        "{}{}",
        resolved_prefix(&identity_expression()?),
        S256::of(FIXTURE)
    );
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    std::fs::write(scene.payload(), FIXTURE)?;
    gh.list_entries(&[("2026-11-04T01:00:00Z", MAIN, &sealed, ENTRY_ID, ENTRY_SIZE)])?;

    gh.lag(2)?;
    run_bind(&scene, &gh, &job, &sealed)?;
    if scene.output_value("bound")? != "true" || !subject.exists() || gh.sleeps() != 2 {
        return Err(fail(&format!(
            "a listing empty twice then complete must bind after two pauses, got {} pauses",
            gh.sleeps()
        )));
    }

    gh.lag(5)?;
    let pauses = gh.sleeps();
    run_bind(&scene, &gh, &job, &sealed)?;
    if scene.output_value("bound")? != "false" || subject.exists() || gh.sleeps() - pauses != 4 {
        return Err(fail(
            "a listing empty five times must refuse after four pauses",
        ));
    }
    scene.finish()
}
