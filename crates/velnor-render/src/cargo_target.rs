// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, Job, RepoPath, StepKind};

use crate::rules::{RenderedStep, Value};

pub(crate) fn hoist_target_dir(job: &Job, steps: &mut [RenderedStep]) -> Vec<(String, Value)> {
    let Some(target) = shared_target_dir(job) else {
        return Vec::new();
    };
    let hoisted = target_dir_env(target);
    for step in steps {
        step.env.retain(|entry| *entry != hoisted);
    }
    vec![hoisted]
}

fn shared_target_dir(job: &Job) -> Option<&RepoPath> {
    if job.steps.iter().any(|step| {
        matches!(
            step.kind,
            StepKind::Cargo(CargoCommand::NextestRunArchive { .. })
        )
    }) {
        return None;
    }
    let mut building = job
        .steps
        .iter()
        .filter(|step| matches!(&step.kind, StepKind::Cargo(command) if command.builds()));
    let first = building.next()?;
    let target = first.target_dir.as_ref()?;
    let one_root = building
        .all(|step| step.target_dir.as_ref() == Some(target) && step.directory == first.directory)
        && job
            .steps
            .iter()
            .all(|step| step.directory.is_none() || step.directory == first.directory);
    one_root.then_some(target)
}

pub(crate) fn target_dir_env(target: &RepoPath) -> (String, Value) {
    (
        "CARGO_TARGET_DIR".to_owned(),
        Value::Expr(format!(
            "${{{{ github.workspace }}}}/{}",
            target.as_str().replace("${{", "${{ '${{' }}")
        )),
    )
}
