// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::state_seal::STATE_SEAL_JOB_ID;
use velnor_actions_contract::{Job, Step, StepRole};

use super::state_producer::{Built, ROOTS, build, job, text_of};
use super::state_producer_eval::{
    HEALTHY, LegFaults, SealLeg, legs_of, producer_outputs, seal_steps_run, simulate_leg,
    writer_roles_run,
};
use super::support::rendered_job_block;

type Artifacts = BTreeMap<(String, String), String>;

fn artifacts(
    built: &Built,
    faults: &LegFaults,
) -> Result<(Vec<String>, Artifacts), Box<dyn Error>> {
    let mut legs = Vec::new();
    let mut merged = Artifacts::new();
    for (producer_id, _) in ROOTS {
        let producer = job(&built.jobs, producer_id)?;
        legs.extend(legs_of(producer));
        merged.extend(producer_outputs(producer_id, producer, faults)?);
    }
    Ok((legs, merged))
}

fn healthy<'a>(
    leg: &'a str,
    artifacts: &'a Artifacts,
    admitted: bool,
    tip_current: bool,
) -> SealLeg<'a> {
    SealLeg {
        leg,
        artifacts,
        sealed: true,
        admitted,
        tip_current,
    }
}

const WRITES: [StepRole; 4] = [
    StepRole::StateSealSave,
    StepRole::StateSealBind,
    StepRole::StateSealAttest,
    StepRole::StateSealRetire,
];

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
    let (legs, artifacts) = artifacts(&built, &HEALTHY)?;
    let leg = legs.first().ok_or("no seal leg")?;
    let admitted = seal_steps_run(seal, &healthy(leg, &artifacts, true, true))?;
    for role in [
        StepRole::StateSealSave,
        StepRole::StateSealBind,
        StepRole::StateSealAttest,
        StepRole::StateSealRetire,
    ] {
        assert!(admitted.contains(&role), "an admitted run skipped {role:?}");
    }
    let refused = seal_steps_run(seal, &healthy(leg, &artifacts, false, true))?;
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
    let (legs, artifacts) = artifacts(&built, &HEALTHY)?;
    for leg in &legs {
        let seal_id = STATE_SEAL_JOB_ID;
        let seal = job(&built.jobs, seal_id)?;
        let current = seal_steps_run(seal, &healthy(leg, &artifacts, true, true))?;
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
        let stale = seal_steps_run(seal, &healthy(leg, &artifacts, true, false))?;
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

#[test]
fn a_stale_head_runs_no_producer_lane_and_uploads_no_payload() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    let stale = LegFaults {
        stale_tip: true,
        ..HEALTHY
    };
    for (producer_id, _) in ROOTS {
        let producer = job(&built.jobs, producer_id)?;
        assert_eq!(
            producer.steps.first().and_then(|step| step.role),
            Some(StepRole::StateSealTip),
            "{producer_id} does not check the tip first"
        );
        for leg in legs_of(producer) {
            let current = simulate_leg(producer, &leg, &HEALTHY)?;
            assert_ne!(current.lanes, 0, "{leg}: a current head ran no lane");
            assert!(current.uploaded, "{leg}: a current head uploaded nothing");
            let stale = simulate_leg(producer, &leg, &stale)?;
            assert_eq!(stale.lanes, 0, "{leg}: a stale head ran lanes");
            assert!(!stale.uploaded, "{leg}: a stale head uploaded a payload");
        }
    }
    Ok(())
}

#[test]
fn a_stale_head_stops_the_sources_and_tools_writers_before_they_fetch_or_save()
-> Result<(), Box<dyn Error>> {
    let built = build()?;
    let writers: Vec<&String> = built
        .jobs
        .keys()
        .filter(|id| id.as_str() == "sources-producer" || id.starts_with("tools-producer-"))
        .collect();
    assert!(writers.iter().any(|id| id.as_str() == "sources-producer"));
    assert!(writers.iter().any(|id| id.starts_with("tools-producer-")));
    let side_effects = [
        StepRole::CargoSourcesRestore,
        StepRole::CargoSourcesFetch,
        StepRole::CargoSourcesSave,
        StepRole::ToolsCacheRestore,
        StepRole::ToolsCacheSave,
        StepRole::PreparePinnedTools,
    ];
    for id in writers {
        let writer = job(&built.jobs, id)?;
        assert_eq!(
            writer.steps.first().and_then(|step| step.role),
            Some(StepRole::StateSealTip),
            "{id} does not check the tip first"
        );
        let current = writer_roles_run(writer, false)?;
        assert!(
            current.contains(&StepRole::Checkout),
            "{id}: a current head skipped checkout"
        );
        assert!(
            side_effects.iter().any(|role| current.contains(role)),
            "{id}: a current head ran no fetch, restore or save"
        );
        let stale = writer_roles_run(writer, true)?;
        for role in side_effects.iter().chain(&[StepRole::Checkout]) {
            assert!(
                !stale.contains(role),
                "{id}: a stale head still ran {role:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_seal_leg_without_its_own_payload_saves_binds_attests_and_retires_nothing()
-> Result<(), Box<dyn Error>> {
    let built = build()?;
    let seal = job(&built.jobs, STATE_SEAL_JOB_ID)?;
    let (legs, _) = artifacts(&built, &HEALTHY)?;
    for faults in [
        LegFaults {
            failing: Some(0),
            ..HEALTHY
        },
        LegFaults {
            refuse_export: true,
            ..HEALTHY
        },
        LegFaults {
            stale_tip: true,
            ..HEALTHY
        },
    ] {
        let (_, artifacts) = artifacts(&built, &faults)?;
        for leg in &legs {
            let ran = seal_steps_run(seal, &healthy(leg, &artifacts, true, true))?;
            assert!(
                !ran.contains(&StepRole::StateSealDownload),
                "{leg}: a leg whose producer uploaded nothing downloaded a payload"
            );
            for role in WRITES {
                assert!(!ran.contains(&role), "{leg}: no payload and {role:?} ran");
            }
        }
    }
    let (_, artifacts) = artifacts(&built, &HEALTHY)?;
    for leg in &legs {
        let refused = SealLeg {
            sealed: false,
            ..healthy(leg, &artifacts, true, true)
        };
        let ran = seal_steps_run(seal, &refused)?;
        for role in WRITES {
            assert!(
                !ran.contains(&role),
                "{leg}: an unsealed payload and {role:?} ran"
            );
        }
    }
    let block = rendered_job_block(text_of(&built, STATE_SEAL_JOB_ID), STATE_SEAL_JOB_ID)?;
    let download = block
        .split("      - name: ")
        .find(|step| step.starts_with("Download sealed payload\n"))
        .ok_or("the rendered seal job lost its download")?;
    assert!(
        download.contains("\n        continue-on-error: true\n"),
        "a missing artifact fails the seal leg: {download}"
    );
    Ok(())
}
