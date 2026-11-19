// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedStep, Value};

pub(crate) fn budget_step(last: usize) -> RenderedStep {
    RenderedStep {
        name: "Check the speed budget".to_owned(),
        env: vec![(
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ github.token }}".to_owned()),
        )],
        run: Some(format!("velnor-ci bench budget --last {last}")),
        ..RenderedStep::default()
    }
}
