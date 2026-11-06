// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{StepKind, StepRole};

use super::{InstallAuditAuthority, audit_prepare_installs_with_catalog_and_verifiers};
use crate::planner::generate::rust_install_audit::tests::make_repo;
use crate::planner::prepare::{GenerationPreparation, prepare_for_candidate as prepare};
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
