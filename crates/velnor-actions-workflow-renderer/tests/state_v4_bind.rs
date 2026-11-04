// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::S256;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_SUBJECT_REL;
use velnor_actions_contract::{Job, StepKind, StepRole};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{ENTRY_ID, ENTRY_SIZE, Gh, MAIN};
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
    let dir = gh.dir();
    let env = [
        ("FAKE_GH_DIR", dir.as_str()),
        ("GITHUB_REF", MAIN),
        ("VELNOR_STATE_SEAL_KEY", key),
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
    scene.finish()
}
