// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_KEY_EXPR;
use velnor_actions_contract::{HomeKey, Job, JobEnv, REQUIRED_JOB_ID, StepRole, WorkflowPolicy};
use velnor_actions_orchestrator::{
    WasteFinding, WasteRule, finalized_jobs, prepare, render_staged_tree,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::one_job_per_crate::{mbx_repo, rust_fixtures, tofu_fixture};
use super::support::rendered_job_block;
use super::tools_producers::{finalized, identity_digest};
use super::waste_graph::{
    clone_crate_job, digests, mbx_jobs, of, recheck_copy, repeat_restore, save_in_crate_job,
};
use super::waste_lint::{home_env, step_name, toolchain_step};
use super::waste_rules::mutable;
use super::waste_tools::{crate_id, foreign_digest, lane_jobs, set_digest};

fn restore_output_job(jobs: &BTreeMap<String, Job>) -> Result<(String, String), Box<dyn Error>> {
    jobs.iter()
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .find(|step| step.role == Some(StepRole::MbxBundleKey))
                .and_then(|step| step.id)
                .map(|step_id| (id.clone(), step_id.as_str().to_owned()))
        })
        .ok_or_else(|| "no job computes the mbx bundle key".into())
}

fn add_key_output(jobs: &mut BTreeMap<String, Job>) -> Result<String, Box<dyn Error>> {
    let (id, step_id) = restore_output_job(jobs)?;
    assert!(MBX_BUNDLE_KEY_EXPR.contains(&format!("steps.{step_id}.outputs")));
    mutable(jobs, &id)?
        .outputs
        .insert("mbx_bundle_key".to_owned(), MBX_BUNDLE_KEY_EXPR.to_owned());
    Ok(id)
}

#[test]
fn w6_flags_a_restore_output_no_other_job_reads() -> Result<(), Box<dyn Error>> {
    let mut all = rust_fixtures()?;
    all.push(tofu_fixture()?);
    for fixture in &all {
        assert_eq!(
            of(&fixture.jobs, WasteRule::UnconsumedRestoreOutput),
            Vec::new(),
            "{}",
            fixture.name
        );
    }
    let repo = mbx_repo(false)?;
    let prepared = prepare(repo.path())?;
    let mut jobs = finalized_jobs(&prepared)?;
    let tree = render_staged_tree(&prepared)?;
    let ci = tree.get(WORKFLOW_PATH).ok_or("CI workflow is missing")?;
    let required = rendered_job_block(ci, REQUIRED_JOB_ID)?;
    assert!(required.contains("toJSON(needs)"), "{required}");
    let id = add_key_output(&mut jobs)?;
    let finding = WasteFinding::UnconsumedOutput {
        job: id.clone(),
        output: "mbx_bundle_key".to_owned(),
    };
    assert_eq!(
        of(&jobs, WasteRule::UnconsumedRestoreOutput),
        vec![finding.clone()]
    );
    let reader = format!("${{{{ needs.{id}.outputs.mbx_bundle_key_other }}}}");
    mutable(&mut jobs, REQUIRED_JOB_ID)?
        .outputs
        .insert("probe".to_owned(), reader);
    assert_eq!(of(&jobs, WasteRule::UnconsumedRestoreOutput), vec![finding]);
    let reader = format!("${{{{ needs.{id}.outputs.mbx_bundle_key }}}}");
    mutable(&mut jobs, REQUIRED_JOB_ID)?
        .outputs
        .insert("probe".to_owned(), reader);
    assert_eq!(of(&jobs, WasteRule::UnconsumedRestoreOutput), Vec::new());
    Ok(())
}

#[test]
fn generation_refuses_a_role_less_copy_of_a_restore() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    let mut prepared = prepare(repo.path())?;
    finalized_jobs(&prepared)?;
    let (id, copy) = prepared
        .workflow
        .ir
        .jobs
        .iter()
        .filter(|(_, job)| job.producer_kind().is_none())
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .find(|step| step.role == Some(StepRole::CargoSourcesRestore))
                .map(|step| (id.clone(), recheck_copy(step)))
        })
        .ok_or("no sources restore in the workflow")?;
    prepared
        .workflow
        .ir
        .jobs
        .get_mut(&id)
        .ok_or("job vanished")?
        .steps
        .push(copy);
    let message = finalized_jobs(&prepared)
        .err()
        .ok_or("a repeated restore generated")?
        .to_string();
    assert!(message.contains("W4"), "{message}");
    assert!(message.contains(&id), "{message}");
    Ok(())
}

struct Scenario {
    jobs: BTreeMap<String, Job>,
    mentions: Vec<String>,
}

fn scenario(rule: WasteRule) -> Result<Scenario, Box<dyn Error>> {
    let mut jobs = mbx_jobs()?;
    let mentions = match rule {
        WasteRule::DuplicateTask => {
            let id = clone_crate_job(&mut jobs)?;
            let digest = digests(&jobs[&id])
                .into_iter()
                .next()
                .ok_or("crate job runs no task")?;
            vec![id, digest]
        }
        WasteRule::DuplicateRestore => {
            let (job, key) = repeat_restore(
                &mut jobs,
                |step| step.role == Some(StepRole::ToolsCacheRestore),
                None,
            )?;
            vec![job, key]
        }
        WasteRule::NonProducerSave => {
            let (job, step) = save_in_crate_job(&mut jobs)?;
            vec![job, step]
        }
        WasteRule::UnconsumedRestoreOutput => {
            vec![add_key_output(&mut jobs)?, "mbx_bundle_key".to_owned()]
        }
        WasteRule::SplitToolsIdentity => {
            let mut jobs = lane_jobs()?;
            let id = crate_id(&jobs)?;
            let own = identity_digest(&jobs[&id])
                .ok_or("crate job has no identity")?
                .to_owned();
            let other = foreign_digest(&jobs, &own)?;
            let runs_on = jobs[&id].runs_on.clone();
            set_digest(jobs.get_mut(&id).ok_or("job vanished")?, &other)?;
            return Ok(Scenario {
                jobs,
                mentions: vec![id, runs_on, own[..8].to_owned(), other[..8].to_owned()],
            });
        }
        WasteRule::JobContextEnv => {
            let (_, mut jobs) = finalized(WorkflowPolicy::ConsumerV1)?;
            let (id, at) = toolchain_step(&jobs)?;
            let step = step_name(&jobs, &id, at);
            let env =
                home_env(&mut mutable(&mut jobs, &id)?.steps[at]).ok_or("not a shell step")?;
            for key in HomeKey::ALL {
                env.remove(key.env_name());
            }
            return Ok(Scenario {
                jobs,
                mentions: vec![id, step],
            });
        }
        WasteRule::RustupAutoInstall => {
            let (_, mut jobs) = finalized(WorkflowPolicy::ConsumerV1)?;
            let (id, _) = toolchain_step(&jobs)?;
            mutable(&mut jobs, &id)?.env = JobEnv::Unset;
            return Ok(Scenario {
                jobs,
                mentions: vec![id],
            });
        }
    };
    Ok(Scenario { jobs, mentions })
}

#[test]
fn every_rule_refuses_a_violation_and_names_it() -> Result<(), Box<dyn Error>> {
    for rule in WasteRule::ALL {
        let Scenario { jobs, mentions } = scenario(*rule)?;
        let findings = of(&jobs, *rule);
        assert!(!findings.is_empty(), "{rule:?}: no finding");
        let text = findings[0].to_string();
        assert!(text.starts_with(rule.id()), "{rule:?}: {text}");
        for mention in mentions {
            assert!(text.contains(&mention), "{rule:?}: {mention} not in {text}");
        }
    }
    Ok(())
}
