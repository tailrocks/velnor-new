// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{StepKind, StepRole};

use super::{InstallAuditAuthority, audit_prepare_installs_with_catalog_and_verifiers};
use crate::generate::rust_install_audit::tests::make_repo;
use crate::prepare::{GenerationPreparation, prepare_for_candidate as prepare};
use crate::workflow::verifier_actions::VerifierActionSet;

const FINDING: &str = "rust_profile_not_minimal";
const MINIMAL: &str = "rust[profile=minimal]@";

fn blocking(prep: &GenerationPreparation) -> Result<Vec<String>, Box<dyn Error>> {
    let verifiers = VerifierActionSet::build(&prep.emission_catalog(), env!("CARGO_PKG_VERSION"))?;
    Ok(audit_prepare_installs_with_catalog_and_verifiers(
        &prep.root,
        &prep.workflow.ir,
        InstallAuditAuthority {
            policy: prep.config.workflow.policy,
            published_generator_lock: None,
            approved_release: None,
        },
        &prep.runner_label,
        &prep.workflow.context.validator_commands,
        &prep.catalog,
        &verifiers,
    )
    .blocking)
}

fn findings(blocking: &[String]) -> Vec<&str> {
    blocking
        .iter()
        .map(String::as_str)
        .filter(|entry| entry.starts_with(FINDING))
        .collect()
}

#[test]
fn a_bare_rust_install_is_a_blocking_lock_audit_finding() -> Result<(), Box<dyn Error>> {
    let repo = make_repo()?;
    let mut prep = prepare(repo.path())?;
    let clean = blocking(&prep)?;
    assert!(findings(&clean).is_empty(), "{clean:?}");
    let mut downgraded = None;
    for (id, job) in &mut prep.workflow.ir.jobs {
        for step in &mut job.steps {
            if step.role != Some(StepRole::PreparePinnedTools) {
                continue;
            }
            let StepKind::Shell { run, .. } = &mut step.kind else {
                continue;
            };
            for token in run.iter_mut().filter(|token| token.starts_with(MINIMAL)) {
                *token = token.replacen(MINIMAL, "rust@", 1);
                downgraded.get_or_insert_with(|| id.clone());
            }
        }
        if downgraded.is_some() {
            break;
        }
    }
    let job = downgraded.ok_or("no job installs Rust through the prepare step")?;
    let dirty = blocking(&prep)?;
    assert_eq!(findings(&dirty), [format!("{FINDING}:{job}")], "{dirty:?}");
    Ok(())
}

#[test]
fn cross_target_verifier_requires_exact_candidate_identity() -> Result<(), Box<dyn Error>> {
    use velnor_actions_contract::WorkflowPolicy;

    let repo = make_repo()?;
    let prep = prepare(repo.path())?;
    let verifiers = VerifierActionSet::build(&prep.emission_catalog(), env!("CARGO_PKG_VERSION"))?;
    let mut job = prep.workflow.ir.jobs.values().next().ok_or("job")?.clone();
    job.steps = vec![verifiers.cross_target_rust.clone()];
    let audit = |candidate: &velnor_actions_contract::Job, id, policy| {
        let mut blocking = Vec::new();
        super::prepare_sequence::verifier_positions(
            id,
            candidate,
            &verifiers,
            policy,
            &mut blocking,
        );
        blocking
    };
    let id = "check-native-image-candidates";
    let policy = WorkflowPolicy::VelnorRepositoryV1;
    assert!(audit(&job, id, policy).is_empty());
    let mut changed = job.clone();
    if let StepKind::Shell { run, .. } = &mut changed.steps[0].kind {
        run[2].push('x');
    } else {
        return Err("expected shell verifier".into());
    }
    assert!(!audit(&changed, id, policy).is_empty());
    changed = job.clone();
    if let StepKind::Shell { env, .. } = &mut changed.steps[0].kind {
        env.insert("CARGO_HOME".to_owned(), "/tmp/other".to_owned());
    }
    assert!(!audit(&changed, id, policy).is_empty());
    changed = job.clone();
    changed.steps[0].condition = Some("always()".to_owned());
    assert!(!audit(&changed, id, policy).is_empty());
    changed = job.clone();
    changed.steps.clear();
    assert!(!audit(&changed, id, policy).is_empty());
    changed = job.clone();
    changed.steps.push(verifiers.cross_target_rust.clone());
    assert!(!audit(&changed, id, policy).is_empty());
    assert!(!audit(&job, id, WorkflowPolicy::ConsumerV1).is_empty());
    assert!(!audit(&job, "another-job", policy).is_empty());
    Ok(())
}
