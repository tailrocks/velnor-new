// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BUDGET_OUTPUTS, BUDGET_STEP, Job, StepKind};

use crate::rules::{RenderedStep, Value};

pub(crate) fn budget_outputs(job: &Job) -> Vec<(String, String)> {
    if !job
        .steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::Budget { .. }))
    {
        return Vec::new();
    }
    BUDGET_OUTPUTS
        .iter()
        .map(|name| {
            (
                name.to_string(),
                format!("${{{{ steps.{BUDGET_STEP}.outputs.{name} }}}}"),
            )
        })
        .collect()
}

pub(crate) fn budget_step(last: usize) -> RenderedStep {
    RenderedStep {
        name: "Check the speed budget".to_owned(),
        id: Some(BUDGET_STEP.to_owned()),
        env: vec![(
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ github.token }}".to_owned()),
        )],
        run: Some(format!("velnor-ci bench budget --last {last}")),
        ..RenderedStep::default()
    }
}
