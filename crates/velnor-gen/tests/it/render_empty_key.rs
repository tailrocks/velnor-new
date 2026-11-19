// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Value, render_workflow};

use crate::rules::plan;

#[test]
fn an_empty_env_key_is_emitted_quoted() -> Result<(), Box<dyn Error>> {
    let rendered = RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "format".to_owned(),
            runs_on: "ubuntu-latest".to_owned(),
            timeout_minutes: 10,
            steps: vec![RenderedStep {
                name: "step".to_owned(),
                run: Some("cargo-fmt --all --check".to_owned()),
                env: vec![(String::new(), Value::Text("x".to_owned()))],
                ..RenderedStep::default()
            }],
            ..RenderedJob::default()
        }],
    };
    let files = render_workflow(&plan(Vec::new())?, &rendered).map_err(|v| format!("{v:?}"))?;
    let text = files.values().next().ok_or("workflow missing")?;
    assert!(text.contains("          '': x\n"), "{text}");
    Ok(())
}
