// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::state_seal::{
    STATE_PRODUCER_ARTIFACT_OUTPUT, STATE_PRODUCER_PRODUCED_OUTPUT,
};
use velnor_actions_contract::{Job, Step, StepRole};

use super::state_producer::{ROOTS, build, job, text_of};
use super::state_producer_eval::{seal_runs, seal_steps_run, simulate};
use super::support::rendered_job_block;

#[test]
fn the_seal_needs_both_the_produced_flag_and_the_artifact_id() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    let (producer_id, seal_id) = ROOTS[0];
    let producer = job(&built.jobs, producer_id)?;
    let seal = job(&built.jobs, seal_id)?;
    let healthy = simulate(producer, None, false)?;
    assert!(
        seal_runs(seal, producer_id, &healthy)?,
        "a healthy producer must seal"
    );
    for (output, label) in [
        (
            STATE_PRODUCER_ARTIFACT_OUTPUT,
            "an artifact id without produced",
        ),
        (
            STATE_PRODUCER_PRODUCED_OUTPUT,
            "produced without an artifact id",
        ),
    ] {
        let mut outputs = healthy.clone();
        outputs.insert(output.to_owned(), String::new());
        assert!(
            !seal_runs(seal, producer_id, &outputs)?,
            "{label}: the seal ran"
        );
    }
    Ok(())
}

#[test]
fn a_seal_job_with_retire_before_save_or_attest_is_refused() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    let (_, seal_id) = ROOTS[0];
    let mut ir = built.ir.clone();
    ir.jobs.clone_from(&built.jobs);
    ir.validate()?;

    let position = |job: &Job, role: StepRole| {
        job.steps
            .iter()
            .position(|step| step.role == Some(role))
            .ok_or("seal job lost a step")
    };
    for ahead_of in [StepRole::StateSealSave, StepRole::StateSealAttest] {
        let mut mutated = ir.clone();
        let seal = mutated.jobs.get_mut(seal_id).ok_or("no seal job")?;
        let retire = seal
            .steps
            .remove(position(seal, StepRole::StateSealRetire)?);
        let at = position(seal, ahead_of)?;
        seal.steps.insert(at, retire);
        let error = mutated
            .validate()
            .err()
            .ok_or("a seal job with retire out of order validated")?;
        assert!(
            error
                .to_string()
                .contains(&format!("mixed_or_invalid_producer:{seal_id}")),
            "{ahead_of:?}: {error}"
        );
    }
    Ok(())
}

#[test]
fn every_rust_job_and_the_producer_end_with_a_stats_step_that_always_runs()
-> Result<(), Box<dyn Error>> {
    let built = build()?;
    let mut checked = 0;
    for (id, candidate) in &built.jobs {
        if !(id.starts_with("rust-") || id.starts_with("state-producer-")) {
            continue;
        }
        let stats: Vec<&Step> = candidate
            .steps
            .iter()
            .filter(|step| step.role == Some(StepRole::MbxStats))
            .collect();
        let [step] = stats[..] else {
            return Err(format!("{id} has {} stats steps", stats.len()).into());
        };
        assert_eq!(step.condition.as_deref(), Some("always()"), "{id}");
        let block = rendered_job_block(text_of(&built, id), id)?;
        let at = block
            .find("      - name: MBX stats\n")
            .ok_or_else(|| format!("{id} renders no stats step"))?;
        let rendered: Vec<&str> = block[at..].lines().skip(1).take(3).collect();
        assert!(
            rendered.contains(&"        if: always()")
                && rendered.contains(&"        continue-on-error: true"),
            "{id}: {rendered:?}"
        );
        assert!(
            block.contains("MBX_SUMMARY: ci") || text_of(&built, id).contains("MBX_SUMMARY: ci"),
            "{id}: no MBX_SUMMARY"
        );
        checked += 1;
    }
    assert!(checked >= 3, "only {checked} jobs were checked");
    Ok(())
}

#[test]
fn a_refused_admit_leaves_save_bind_and_attest_unrun() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    let (_, seal_id) = ROOTS[0];
    let seal = job(&built.jobs, seal_id)?;
    let admitted = seal_steps_run(seal, true, true)?;
    for role in [
        StepRole::StateSealSave,
        StepRole::StateSealBind,
        StepRole::StateSealAttest,
        StepRole::StateSealRetire,
    ] {
        assert!(admitted.contains(&role), "an admitted run skipped {role:?}");
    }
    let refused = seal_steps_run(seal, false, true)?;
    assert!(refused.contains(&StepRole::StateSealAdmit));
    for role in [
        StepRole::StateSealSave,
        StepRole::StateSealBind,
        StepRole::StateSealAttest,
        StepRole::StateSealRetire,
    ] {
        assert!(
            !refused.contains(&role),
            "a refused admit still ran {role:?}"
        );
    }
    Ok(())
}

#[test]
fn a_stale_head_saves_binds_and_attests_nothing() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    for (_, seal_id) in ROOTS {
        let seal = job(&built.jobs, seal_id)?;
        let current = seal_steps_run(seal, true, true)?;
        for role in [
            StepRole::StateSealTip,
            StepRole::StateSealAdmit,
            StepRole::StateSealSave,
            StepRole::StateSealBind,
            StepRole::StateSealAttest,
            StepRole::StateSealRetire,
        ] {
            assert!(
                current.contains(&role),
                "{seal_id}: a current head skipped {role:?}"
            );
        }
        let stale = seal_steps_run(seal, true, false)?;
        assert!(stale.contains(&StepRole::StateSealTip), "{seal_id}");
        for role in [
            StepRole::StateSealAdmit,
            StepRole::StateSealSave,
            StepRole::StateSealBind,
            StepRole::StateSealAttest,
            StepRole::StateSealRetire,
        ] {
            assert!(
                !stale.contains(&role),
                "{seal_id}: a stale head still ran {role:?}"
            );
        }
    }
    Ok(())
}
