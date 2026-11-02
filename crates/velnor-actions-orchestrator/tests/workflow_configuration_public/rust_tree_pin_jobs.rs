// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use sha2::{Digest, Sha256};
use velnor_actions_contract::workflow::RUST_VERIFIER_USES;
use velnor_actions_contract::{Job, Step, StepKind, StepRole};
use velnor_actions_mise::{RUST_TARGET_TRIPLE, RUST_VERSION};
use velnor_actions_workflow_renderer::verifier_builders::RUST_TREE_DIGESTS;

use super::install_profile::rust_install_profile;
use super::tools_producers::{POLICIES, finalized, positions};

fn is_restored_form(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. } if uses == RUST_VERIFIER_USES[0])
}

fn installed_verifiers(job: &Job) -> Vec<usize> {
    let Some(install) = positions(job, StepRole::PreparePinnedTools)
        .first()
        .copied()
    else {
        return Vec::new();
    };
    positions(job, StepRole::VerifyRustManifest)
        .into_iter()
        .filter(|at| *at > install && !is_restored_form(&job.steps[*at]))
        .collect()
}

fn added_components(step: &Step) -> Vec<String> {
    let StepKind::Shell { run, .. } = &step.kind else {
        return Vec::new();
    };
    run.iter()
        .skip_while(|token| token.as_str() != "--toolchain")
        .skip(2)
        .cloned()
        .collect()
}

fn profile_components(job: &Job) -> Result<[&'static str; 3], Box<dyn Error>> {
    match rust_install_profile(job).as_deref() {
        Some("minimal") => Ok(["cargo", "rustc", "rust-std"]),
        other => Err(format!("unknown Rust install profile {other:?}").into()),
    }
}

fn final_component_names(job: &Job) -> Result<Vec<String>, Box<dyn Error>> {
    let added = positions(job, StepRole::PrepareRustComponents)
        .into_iter()
        .flat_map(|at| added_components(&job.steps[at]))
        .map(|name| format!("{name}-preview"));
    let mut names = profile_components(job)?
        .into_iter()
        .map(str::to_owned)
        .chain(added)
        .map(|name| format!("{name}-{RUST_TARGET_TRIPLE}\n"))
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

fn final_components_sha256(job: &Job) -> Result<String, Box<dyn Error>> {
    Ok(
        Sha256::digest(final_component_names(job)?.concat().as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .concat(),
    )
}

fn final_tree_is_reverified(job: &Job) -> bool {
    let Some(last_add) = positions(job, StepRole::PrepareRustComponents)
        .into_iter()
        .max()
    else {
        return true;
    };
    let first_cargo = job
        .steps
        .iter()
        .position(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
        .unwrap_or(job.steps.len());
    installed_verifiers(job)
        .into_iter()
        .any(|at| at > last_add && at < first_cargo)
}

fn finalized_jobs() -> Result<Vec<(String, Job)>, Box<dyn Error>> {
    let mut jobs = Vec::new();
    for policy in POLICIES {
        let (_, finalized) = finalized(policy)?;
        jobs.extend(finalized);
    }
    Ok(jobs)
}

fn installs_rust(job: &Job) -> bool {
    !installed_verifiers(job).is_empty()
        || !positions(job, StepRole::PrepareRustComponents).is_empty()
}

fn rust_jobs(jobs: Vec<(String, Job)>) -> Vec<(String, Job)> {
    jobs.into_iter()
        .filter(|(_, job)| installs_rust(job))
        .collect()
}

fn rendered_rust_jobs() -> Result<Vec<(String, Job)>, Box<dyn Error>> {
    Ok(rust_jobs(finalized_jobs()?))
}

fn not_reverified(jobs: &[(String, Job)]) -> Vec<usize> {
    jobs.iter()
        .enumerate()
        .filter(|(_, (_, job))| !final_tree_is_reverified(job))
        .map(|(at, _)| at)
        .collect()
}

#[test]
fn pinned_component_sets_match_the_sets_generated_jobs_install() -> Result<(), Box<dyn Error>> {
    let produced = rendered_rust_jobs()?
        .iter()
        .map(|(_, job)| final_components_sha256(job))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let pinned = RUST_TREE_DIGESTS
        .iter()
        .filter(|pin| pin.version() == RUST_VERSION && pin.target() == RUST_TARGET_TRIPLE)
        .map(|pin| pin.components_sha256().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(produced.len(), 2, "{produced:?}");
    assert_eq!(produced, pinned);
    Ok(())
}

#[test]
fn installed_verifier_follows_the_last_component_add_before_any_cargo_use()
-> Result<(), Box<dyn Error>> {
    let jobs = rendered_rust_jobs()?;
    let with_adds = jobs
        .iter()
        .filter(|(_, job)| !positions(job, StepRole::PrepareRustComponents).is_empty())
        .collect::<Vec<_>>();
    assert!(with_adds.iter().any(|(_, job)| {
        job.steps
            .iter()
            .any(|s| matches!(s.kind, StepKind::TaskExecution { .. }))
    }));
    let broken = not_reverified(&jobs)
        .into_iter()
        .map(|at| jobs[at].0.as_str())
        .collect::<Vec<_>>();
    assert!(broken.is_empty(), "{broken:?}");
    Ok(())
}

#[test]
fn a_component_adding_job_without_any_installed_verifier_is_checked() -> Result<(), Box<dyn Error>>
{
    let mut jobs = finalized_jobs()?;
    let victim = jobs
        .iter()
        .position(|(_, job)| {
            !installed_verifiers(job).is_empty()
                && !positions(job, StepRole::PrepareRustComponents).is_empty()
        })
        .ok_or("no rendered job adds components after an installed verifier")?;
    let before = not_reverified(&rust_jobs(jobs.clone()));
    assert!(before.is_empty(), "{before:?}");
    for at in installed_verifiers(&jobs[victim].1).into_iter().rev() {
        jobs[victim].1.steps.remove(at);
    }
    let broken = rust_jobs(jobs);
    let flagged = not_reverified(&broken)
        .into_iter()
        .map(|at| broken[at].0.clone())
        .collect::<Vec<_>>();
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    Ok(())
}

type Mutate = fn(&mut Job, usize, usize);

fn mutations() -> [(&'static str, Mutate); 4] {
    [
        ("verifier removed", |job, _, verify| {
            job.steps.remove(verify);
        }),
        ("verifier before the add", |job, add, verify| {
            let step = job.steps.remove(verify);
            job.steps.insert(add, step);
        }),
        ("verifier is the restored form", |job, _, verify| {
            job.steps[verify].kind = StepKind::Action {
                uses: RUST_VERIFIER_USES[0].to_owned(),
                with: std::collections::BTreeMap::new(),
                env: std::collections::BTreeMap::new(),
            };
        }),
        ("verifier after cargo use", |job, _, verify| {
            let step = job.steps.remove(verify);
            let cargo = job
                .steps
                .iter()
                .position(|s| matches!(s.kind, StepKind::TaskExecution { .. }))
                .unwrap_or(job.steps.len());
            job.steps.insert(cargo + 1, step);
        }),
    ]
}

#[test]
fn a_job_without_a_reverified_final_tree_is_detected() -> Result<(), Box<dyn Error>> {
    let (id, job) = rendered_rust_jobs()?
        .into_iter()
        .find(|(_, job)| {
            !positions(job, StepRole::PrepareRustComponents).is_empty()
                && job
                    .steps
                    .iter()
                    .any(|s| matches!(s.kind, StepKind::TaskExecution { .. }))
        })
        .ok_or("no rendered crate job installs components")?;
    assert!(final_tree_is_reverified(&job), "{id}");
    let add = positions(&job, StepRole::PrepareRustComponents)[0];
    let verify = installed_verifiers(&job)[0];
    for (name, mutate) in mutations() {
        let mut broken = job.clone();
        mutate(&mut broken, add, verify);
        assert!(!final_tree_is_reverified(&broken), "{id}: {name}");
    }
    Ok(())
}
