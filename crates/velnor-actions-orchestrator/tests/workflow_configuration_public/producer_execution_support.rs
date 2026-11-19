// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{GenerationPreparation, render_staged_tree};

pub(super) fn assert_conflicting_task_rejected(
    prepared: &GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    let mut conflicting_prepared = prepared.clone();
    let job = conflicting_prepared
        .workflow
        .ir
        .jobs
        .iter_mut()
        .find(|(id, _)| id.starts_with("state-producer-"))
        .map(|(_, job)| job)
        .ok_or("producer missing")?;
    let mut conflicting = job
        .steps
        .iter()
        .find(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
        .ok_or("producer task missing")?
        .clone();
    let StepKind::TaskExecution { env, .. } = &mut conflicting.kind else {
        return Err("producer task missing".into());
    };
    env.insert("PRODUCER_CONFLICT".to_owned(), "different".to_owned());
    job.steps.push(conflicting);
    assert!(
        render_staged_tree(&conflicting_prepared).is_err(),
        "conflicting producer task was accepted"
    );
    Ok(())
}
