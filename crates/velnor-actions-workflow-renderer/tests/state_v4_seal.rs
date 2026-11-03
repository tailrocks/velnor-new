// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily, S256,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_CANARY_SEAL_JOB_ID, STATE_SEAL_KEY_EXPR, STATE_SEAL_PREFIX_ENV, STATE_SEAL_SUBJECT_REL,
    state_canary_identity_prefix,
};
use velnor_actions_contract::{CacheAccess, Job, JobEnv, JobTimeout, Step, StepKind, StepRole};
use velnor_actions_workflow_renderer::plan_sources::state_select::state_select_step;
use velnor_actions_workflow_renderer::state_seal::add_state_seal_canary;

use crate::gc1_scene::{Scene, fail};

const FIXTURE: &[u8] = b"sealed state payload fixture";

fn plan_job() -> Result<Job, Box<dyn Error>> {
    Ok(Job {
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_access: CacheAccess::Read,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: JobEnv::Unset,
        steps: vec![state_select_step(&[], true)?],
    })
}

fn seal_job() -> Result<Job, Box<dyn Error>> {
    let mut jobs = BTreeMap::from([("plan".to_owned(), plan_job()?)]);
    add_state_seal_canary(&mut jobs)?;
    jobs.remove(STATE_CANARY_SEAL_JOB_ID)
        .ok_or_else(|| fail("the seal job was not generated"))
}

fn step_of(job: &Job, role: StepRole) -> Result<&Step, Box<dyn Error>> {
    job.steps
        .iter()
        .find(|step| step.role == Some(role))
        .ok_or_else(|| fail(&format!("seal job lost its {role:?} step")))
}

fn inputs_of(step: &Step) -> Result<&BTreeMap<String, String>, Box<dyn Error>> {
    match &step.kind {
        StepKind::Action { with, .. } => Ok(with),
        _ => Err(fail("expected an action step")),
    }
}

fn on_runner(scene: &Scene, expression: &str) -> String {
    expression.replace("${{ runner.temp }}", &scene.runner.display().to_string())
}

fn run_subject(scene: &Scene, job: &Job) -> Result<String, Box<dyn Error>> {
    let step = step_of(job, StepRole::StateSealSubject)?;
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(fail("the subject step is not a shell step"));
    };
    match std::fs::remove_file(&scene.github_output) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let pairs: Vec<(&str, &str)> = env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    scene.run_script(run.get(2).ok_or_else(|| fail("no script"))?, &pairs, None)
}

fn assert_refused(scene: &Scene, output: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if scene.output_value("sealed")? != "false" {
        return Err(fail(&format!("{label}: the payload was sealed")));
    }
    if scene.runner.join(STATE_SEAL_SUBJECT_REL).exists() {
        return Err(fail(&format!("{label}: a subject was written: {output}")));
    }
    Ok(())
}

fn reset_payload(scene: &Scene) -> Result<(), Box<dyn Error>> {
    let path = scene.payload();
    if std::fs::symlink_metadata(&path).is_ok() {
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    std::fs::create_dir_all(path.parent().ok_or_else(|| fail("no payload parent"))?)?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn attest_job_binds_key_to_stored_file() -> Result<(), Box<dyn Error>> {
    let job = seal_job()?;
    let scene = Scene::new("seal-attest", crate::gc1_t53::scripts()?)?;
    let subject_step = step_of(&job, StepRole::StateSealSubject)?;
    let StepKind::Shell { env, .. } = &subject_step.kind else {
        return Err(fail("the subject step is not a shell step"));
    };
    let prefix = env
        .get(STATE_SEAL_PREFIX_ENV)
        .ok_or_else(|| fail("the subject step lost its identity prefix"))?;
    if *prefix != state_canary_identity_prefix() {
        return Err(fail(
            "the key identity does not come from the planned constants",
        ));
    }

    reset_payload(&scene)?;
    std::fs::write(scene.payload(), FIXTURE)?;
    run_subject(&scene, &job)?;
    let want = format!("{prefix}{}", S256::of(FIXTURE));
    if scene.output_value("sealed")? != "true" || scene.output_value("key")? != want {
        return Err(fail(
            "the key is not the identity plus the stored file digest",
        ));
    }
    let subject = scene.runner.join(STATE_SEAL_SUBJECT_REL);
    if std::fs::read(&subject)? != format!("{want}\n").as_bytes() {
        return Err(fail("the subject bytes are not the key and one line feed"));
    }
    let save = inputs_of(step_of(&job, StepRole::StateSealSave)?)?;
    if save.get("key").map(String::as_str) != Some(STATE_SEAL_KEY_EXPR)
        || save.get("path").map(String::as_str) != Some(COMPLETE_STATE_TRANSPORT_EXPR)
        || Path::new(&on_runner(&scene, COMPLETE_STATE_TRANSPORT_EXPR)) != scene.payload()
    {
        return Err(fail(
            "the save does not name the downloaded file and the sealed key",
        ));
    }
    let attest = inputs_of(step_of(&job, StepRole::StateSealAttest)?)?;
    let attested = attest
        .get("subject-path")
        .map(|expression| on_runner(&scene, expression));
    if attested.as_deref().map(Path::new) != Some(subject.as_path()) {
        return Err(fail(
            "the attestation does not cover the subject the shell wrote",
        ));
    }

    let ceiling = CacheFamily::MbxBundle.spec().ceiling.stored();
    reset_payload(&scene)?;
    std::fs::File::create(scene.payload())?.set_len(ceiling + 1)?;
    let output = run_subject(&scene, &job)?;
    assert_refused(&scene, &output, "over the stored ceiling")?;
    let both = format!("over_stored stored={} ceiling={ceiling}", ceiling + 1);
    if !output.contains(&both) {
        return Err(fail(&format!("the refusal lost its numbers: {output}")));
    }

    reset_payload(&scene)?;
    std::fs::write(scene.payload(), b"")?;
    let output = run_subject(&scene, &job)?;
    assert_refused(&scene, &output, "empty file")?;

    reset_payload(&scene)?;
    std::fs::write(scene.scratch.join("elsewhere"), FIXTURE)?;
    std::os::unix::fs::symlink(scene.scratch.join("elsewhere"), scene.payload())?;
    let output = run_subject(&scene, &job)?;
    assert_refused(&scene, &output, "symlink")?;

    reset_payload(&scene)?;
    std::fs::create_dir(scene.payload())?;
    let output = run_subject(&scene, &job)?;
    assert_refused(&scene, &output, "directory")?;
    scene.finish()
}

#[cfg(target_os = "linux")]
fn install_decode_recorder(scene: &Scene) -> Result<(), Box<dyn Error>> {
    let path = std::env::var("PATH")?;
    let real = std::env::split_paths(&path)
        .map(|dir| dir.join("zstd"))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| fail("no zstd on PATH"))?;
    scene.install_shim(
        "zstd",
        &format!(
            "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in -d|-dc|-cd) printf 'decode\\n' >> \"$ZSTD_LOG\" ;; esac; done\nexec {} \"$@\"\n",
            real.display()
        ),
    )
}

#[test]
#[cfg(target_os = "linux")]
fn recover_refuses_payload_that_differs_from_the_key_digest() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-digest", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    install_decode_recorder(&scene)?;
    let log = scene.scratch.join("zstd.log");
    let log_text = log.display().to_string();
    let env = [("ZSTD_LOG", log_text.as_str())];

    scene.pack_export("sealed")?;
    let sealed = format!("{}{}", key.snapshot_prefix, scene.payload_digest()?);
    scene.recover_unbound(&sealed, &env, None)?;
    if scene.store_files()?.is_empty() || !scene.recovery_record().exists() {
        return Err(fail(
            "a payload that matches its key digest was not imported",
        ));
    }

    std::fs::remove_file(scene.recovery_record())?;
    std::fs::remove_dir_all(scene.store())?;
    std::fs::create_dir_all(scene.store().join("actions"))?;
    scene.pack_export("swapped")?;
    let _ = std::fs::remove_file(&log);
    let output = scene.recover_unbound(&sealed, &env, None)?;
    scene.assert_cold(&output, "refused-digest")?;
    if std::fs::read_to_string(&log).is_ok_and(|text| text.contains("decode")) {
        return Err(fail(
            "the decoder ran on a payload that does not match its key",
        ));
    }

    scene.pack_export("sealed")?;
    let undigested = format!("{}oldsnap", key.snapshot_prefix);
    let output = scene.recover_unbound(&undigested, &env, None)?;
    scene.assert_cold(&output, "refused-digest")?;
    scene.finish()
}
