// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::{HomeKey, Job, JobEnv, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_orchestrator::{WasteFinding, waste_findings};

use super::waste_lint::{home_env, plain_shell_step, step_name};

fn shell_step_with_role(
    jobs: &BTreeMap<String, Job>,
    role: StepRole,
    installs_rust: bool,
) -> Option<(String, usize)> {
    jobs.iter()
        .filter(|(_, job)| (job.env != JobEnv::Unset) == installs_rust)
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .position(|step| {
                    step.role == Some(role) && matches!(step.kind, StepKind::Shell { .. })
                })
                .map(|at| (id.clone(), at))
        })
}

pub(super) fn assert_homes_are_enforced(
    jobs: &mut BTreeMap<String, Job>,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let (id, at) = shell_step_with_role(jobs, StepRole::VerifyRustManifest, true)
        .ok_or("no rust-installing job has a shell Rust verifier step")?;
    let name = step_name(jobs, &id, at);
    let baseline = waste_findings(jobs);
    let env = home_env(&mut jobs.get_mut(&id).ok_or("job vanished")?.steps[at])
        .ok_or("not a shell step")?;
    let saved = env.clone();
    env.remove("RUSTUP_HOME");
    env.remove("CARGO_HOME");
    assert_eq!(
        waste_findings(jobs),
        vec![WasteFinding::MissingVerifierHome {
            job: id.clone(),
            step: name.clone(),
            missing: vec![HomeKey::CargoHome, HomeKey::RustupHome],
        }],
        "{policy:?}"
    );
    let env = home_env(&mut jobs.get_mut(&id).ok_or("job vanished")?.steps[at])
        .ok_or("not a shell step")?;
    env.extend(saved.clone());
    env.insert("RUSTUP_HOME".to_owned(), "/tmp/other".to_owned());
    assert_eq!(
        waste_findings(jobs),
        vec![WasteFinding::ConflictingHome {
            job: id.clone(),
            step: name,
            key: HomeKey::RustupHome,
            expected: HomeKey::RustupHome.standard_value().to_owned(),
            found: "/tmp/other".to_owned(),
        }],
        "{policy:?}"
    );
    let env = home_env(&mut jobs.get_mut(&id).ok_or("job vanished")?.steps[at])
        .ok_or("not a shell step")?;
    *env = saved;
    assert_eq!(waste_findings(jobs), baseline, "{policy:?}");
    Ok(())
}

pub(super) fn assert_rule_stays_in_scope(
    jobs: &mut BTreeMap<String, Job>,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let before = waste_findings(jobs);
    let (id, at) = shell_step_with_role(jobs, StepRole::VerifyJactionlint, true)
        .ok_or("no rust-installing job has a shell jactionlint verifier step")?;
    let env = home_env(&mut jobs.get_mut(&id).ok_or("job vanished")?.steps[at])
        .ok_or("not a shell step")?;
    env.clear();
    assert_eq!(waste_findings(jobs), before, "{policy:?}: jactionlint");
    let (id, at) = shell_step_with_role(jobs, StepRole::VerifyRustManifest, true)
        .ok_or("no rust-installing job has a shell Rust verifier step")?;
    let mut stripped = jobs[&id].steps[at].clone();
    home_env(&mut stripped).ok_or("not a shell step")?.clear();
    let (plain_id, _) = plain_shell_step(jobs)?;
    jobs.get_mut(&plain_id)
        .ok_or("job vanished")?
        .steps
        .push(stripped);
    assert_eq!(waste_findings(jobs), before, "{policy:?}: job without rust");
    Ok(())
}
