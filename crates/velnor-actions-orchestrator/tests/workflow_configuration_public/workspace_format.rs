// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_workflow_renderer::plan_format::outcome_format_step;

#[test]
fn workspace_format_outcome_emitter_keeps_native_mise_admission() {
    let error = outcome_format_step(
        vec!["sh".to_owned(), "-c".to_owned(), "mise fmt".to_owned()],
        "m-0123456789abcdef",
        &BTreeMap::new(),
    )
    .expect_err("the outcome wrapper must not bypass native Mise admission");
    assert!(matches!(
        error,
        velnor_actions_workflow_renderer::RenderError::BadCommand(problem)
            if problem == "format_without_mise"
    ));
}
