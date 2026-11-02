// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::ledger_cli::LEDGER_CLI_JOB_ID;
use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, HomeKey, Job, JobEnv, PLAN_JOB_ID, REQUIRED_JOB_ID, StepKind, StepRole,
    WorkflowPolicy,
};
use velnor_actions_orchestrator::{
    ExecWord, WasteFinding, finalized_jobs, prepare, render_staged_tree, waste_findings,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::owner_feature_repo;
use super::support::rendered_job_block;
use super::tools_producers::{POLICIES, finalized};
use super::tools_producers_donors::{SCHEDULED_SUITES_CONFIG, prepared_for};
use super::workflow_configuration_fixture;

const SUITES_PATH: &str = ".github/workflows/suites.yml";

const AUTO_INSTALL_ENV: &str = "RUSTUP_AUTO_INSTALL";

fn children(text: &str) -> Vec<(String, String)> {
    let mut lines = text.lines();
    let head = lines
        .next()
        .map_or(0, |line| line.len() - line.trim_start().len());
    lines
        .take_while(|line| line.len() - line.trim_start().len() > head)
        .filter_map(|line| line.trim().split_once(": "))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn job_env_entries(workflow: &str, block: &str) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let Some(at) = block.find("\n    env:") else {
        return Ok(Vec::new());
    };
    let from_head = &block[at + 1..];
    let value = from_head
        .lines()
        .next()
        .and_then(|line| line.trim().strip_prefix("env:"))
        .map_or("", str::trim);
    let Some(alias) = value.strip_prefix('*') else {
        return Ok(children(from_head));
    };
    let marker = format!("env: &{alias}\n");
    let found = workflow
        .find(&marker)
        .ok_or_else(|| format!("env anchor {alias} is not defined"))?;
    let start = workflow[..found]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    Ok(children(&workflow[start..]))
}

fn auto_install_off(entries: &[(String, String)]) -> bool {
    entries
        .iter()
        .any(|(key, value)| key == AUTO_INSTALL_ENV && value == "\"0\"")
}

fn job_ids(workflow: &str) -> Vec<&str> {
    workflow
        .lines()
        .skip_while(|line| *line != "jobs:")
        .skip(1)
        .filter(|line| line.starts_with("  ") && !line.starts_with("   "))
        .filter_map(|line| line.trim().strip_suffix(':'))
        .collect()
}

fn bound_ids(jobs: &BTreeMap<String, Job>) -> Vec<&str> {
    jobs.iter()
        .filter(|(_, job)| job.env != JobEnv::Unset)
        .map(|(id, _)| id.as_str())
        .collect()
}

#[test]
fn rust_bearing_jobs_render_auto_install_off_at_job_level() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let prepared = prepare(repo.path())?;
        let jobs = finalized_jobs(&prepared)?;
        let tree = render_staged_tree(&prepared)?;
        let ci = tree.get(WORKFLOW_PATH).ok_or("CI workflow is missing")?;
        let bound = bound_ids(&jobs);
        assert!(bound.len() >= 3, "{policy:?}: only {bound:?} are bound");
        for id in job_ids(ci) {
            let entries = job_env_entries(ci, rendered_job_block(ci, id)?)?;
            assert_eq!(
                auto_install_off(&entries),
                bound.contains(&id),
                "{policy:?} {id}: rendered job env {entries:?}"
            );
        }
        if policy == WorkflowPolicy::VelnorRepositoryV1 {
            assert!(bound.contains(&PLAN_JOB_ID), "{bound:?}");
            assert!(bound.contains(&LEDGER_CLI_JOB_ID), "{bound:?}");
            assert!(
                bound.iter().any(|id| id.starts_with(CRATE_JOB_ID_PREFIX)),
                "{bound:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn suites_jobs_that_install_rust_render_auto_install_off_at_job_level() -> Result<(), Box<dyn Error>>
{
    let repo = workflow_configuration_fixture::make_repo(SCHEDULED_SUITES_CONFIG)?;
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let suites = tree.get(SUITES_PATH).ok_or("suites workflow is missing")?;
    let installing: Vec<&str> = job_ids(suites)
        .into_iter()
        .filter(|id| {
            rendered_job_block(suites, id).is_ok_and(|block| {
                block.contains(" install 'rust[") || block.contains(" install rust@")
            })
        })
        .collect();
    assert!(!installing.is_empty(), "no suites job installs rust");
    for id in installing {
        let entries = job_env_entries(suites, rendered_job_block(suites, id)?)?;
        assert!(auto_install_off(&entries), "{id}: {entries:?}");
    }
    Ok(())
}

fn home_env(step: &mut velnor_actions_contract::Step) -> Option<&mut BTreeMap<String, String>> {
    match &mut step.kind {
        StepKind::Shell { env, .. } => Some(env),
        _ => None,
    }
}

fn toolchain_step(jobs: &BTreeMap<String, Job>) -> Result<(String, usize), Box<dyn Error>> {
    jobs.iter()
        .filter(|(_, job)| job.env != JobEnv::Unset)
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .position(|step| {
                    step.role == Some(StepRole::PreparePinnedTools)
                        && matches!(&step.kind, StepKind::Shell { env, .. }
                            if HomeKey::ALL.iter().all(|key| env.contains_key(key.env_name())))
                })
                .map(|at| (id.clone(), at))
        })
        .ok_or_else(|| "no bound job has a toolchain step carrying the homes".into())
}

fn step_name(jobs: &BTreeMap<String, Job>, id: &str, at: usize) -> String {
    jobs[id].steps[at].name.clone()
}

#[test]
fn w2_names_a_toolchain_step_without_the_homes_and_a_wrong_home() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let (_, mut jobs) = finalized(policy)?;
        assert_eq!(waste_findings(&jobs), Vec::new(), "{policy:?}");
        let (id, at) = toolchain_step(&jobs)?;
        let name = step_name(&jobs, &id, at);
        let env = home_env(&mut jobs.get_mut(&id).ok_or("job vanished")?.steps[at])
            .ok_or("not a shell step")?;
        env.insert("CARGO_HOME".to_owned(), "/tmp/other".to_owned());
        assert_eq!(
            waste_findings(&jobs),
            vec![WasteFinding::ConflictingHome {
                job: id.clone(),
                step: name.clone(),
                key: HomeKey::CargoHome,
                expected: "${{ runner.temp }}/velnor/cargo".to_owned(),
                found: "/tmp/other".to_owned(),
            }],
            "{policy:?}"
        );
        let env = home_env(&mut jobs.get_mut(&id).ok_or("job vanished")?.steps[at])
            .ok_or("not a shell step")?;
        for key in HomeKey::ALL {
            env.remove(key.env_name());
        }
        assert_eq!(
            waste_findings(&jobs),
            vec![WasteFinding::MissingHome {
                job: id,
                step: name,
                word: ExecWord::Mise,
                missing: HomeKey::ALL.to_vec(),
            }],
            "{policy:?}"
        );
    }
    Ok(())
}

fn plain_shell_step(jobs: &BTreeMap<String, Job>) -> Result<(String, usize), Box<dyn Error>> {
    jobs.iter()
        .filter(|(_, job)| job.env == JobEnv::Unset)
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .position(|step| matches!(step.kind, StepKind::Shell { .. }))
                .map(|at| (id.clone(), at))
        })
        .ok_or_else(|| "no unbound job has a shell step".into())
}

fn set_auto_install(
    jobs: &mut BTreeMap<String, Job>,
    id: &str,
    at: usize,
    value: &str,
) -> Result<(), Box<dyn Error>> {
    let env = home_env(&mut jobs.get_mut(id).ok_or("job vanished")?.steps[at])
        .ok_or("not a shell step")?;
    env.insert(AUTO_INSTALL_ENV.to_owned(), value.to_owned());
    Ok(())
}

#[test]
fn w3_flags_a_rust_job_without_auto_install_off_and_any_step_override() -> Result<(), Box<dyn Error>>
{
    for policy in POLICIES {
        let (_, mut jobs) = finalized(policy)?;
        let (rust_id, rust_at) = toolchain_step(&jobs)?;
        let (plain_id, plain_at) = plain_shell_step(&jobs)?;
        let plain_name = step_name(&jobs, &plain_id, plain_at);
        set_auto_install(&mut jobs, &plain_id, plain_at, "0")?;
        assert_eq!(waste_findings(&jobs), Vec::new(), "{policy:?}");
        set_auto_install(&mut jobs, &plain_id, plain_at, "1")?;
        assert_eq!(
            waste_findings(&jobs),
            vec![WasteFinding::AutoInstallNotDisabled {
                job: plain_id,
                step: Some(plain_name),
                found: Some("1".to_owned()),
            }],
            "{policy:?}"
        );
        let rust_name = step_name(&jobs, &rust_id, rust_at);
        set_auto_install(&mut jobs, &rust_id, rust_at, "1")?;
        assert!(
            waste_findings(&jobs).contains(&WasteFinding::AutoInstallNotDisabled {
                job: rust_id.clone(),
                step: Some(rust_name),
                found: Some("1".to_owned()),
            }),
            "{policy:?}"
        );
        jobs.get_mut(&rust_id).ok_or("job vanished")?.env = JobEnv::Unset;
        assert!(
            waste_findings(&jobs).contains(&WasteFinding::AutoInstallNotDisabled {
                job: rust_id,
                step: None,
                found: None,
            }),
            "{policy:?}"
        );
    }
    Ok(())
}

#[test]
fn a_step_level_auto_install_override_fails_generation_and_names_the_job()
-> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let (_repo, mut prepared) = prepared_for(policy)?;
        finalized_jobs(&prepared)?;
        let mut edited = Vec::new();
        for (id, job) in &mut prepared.workflow.ir.jobs {
            if id == PLAN_JOB_ID || id == REQUIRED_JOB_ID || edited.len() == 2 {
                continue;
            }
            let Some(step) = job.steps.iter_mut().find_map(|step| {
                let name = step.name.clone();
                home_env(step).map(|env| {
                    env.insert(AUTO_INSTALL_ENV.to_owned(), "1".to_owned());
                    name
                })
            }) else {
                continue;
            };
            edited.push((id.clone(), step));
        }
        assert_eq!(edited.len(), 2, "{policy:?}: not enough shell steps");
        let message = finalized_jobs(&prepared)
            .err()
            .ok_or("an auto-install override generated")?
            .to_string();
        assert!(message.contains("W3"), "{policy:?}: {message}");
        assert!(message.contains("fix:"), "{policy:?}: {message}");
        for (id, step) in edited {
            assert!(message.contains(&id), "{policy:?}: {id} not in {message}");
            assert!(
                message.contains(&step),
                "{policy:?}: {step} not in {message}"
            );
        }
    }
    Ok(())
}
