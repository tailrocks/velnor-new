// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BUDGET_STEP_NAME, SelectionName};

use crate::rules::{RenderedStep, Value};
use crate::selection_name::step_title;

pub(crate) fn budget_step(last: usize, selection: Option<&SelectionName>) -> RenderedStep {
    RenderedStep {
        name: BUDGET_STEP_NAME.to_owned(),
        name_expression: selection.map(step_title),
        env: vec![(
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ github.token }}".to_owned()),
        )],
        run: Some(format!("velnor-ci bench budget --last {last}")),
        ..RenderedStep::default()
    }
}
