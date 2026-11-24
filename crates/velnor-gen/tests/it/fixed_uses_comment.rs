// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_render::{PINNED_ACTIONS, RenderedJob, RenderedWorkflow, render_workflow};

use crate::rules::plan;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn every_pinned_action_renders_its_tag_as_a_trailing_comment_from_its_pin() -> TestResult {
    for pin in PINNED_ACTIONS {
        let workflow = RenderedWorkflow {
            jobs: vec![RenderedJob {
                id: "publish".to_owned(),
                runs_on: "ubuntu-latest".to_owned(),
                timeout_minutes: 10,
                steps: vec![pin.step("step".to_owned())],
                ..RenderedJob::default()
            }],
        };
        let files =
            render_workflow(&plan(Vec::new())?, &workflow).map_err(|found| format!("{found:?}"))?;
        let text = files.values().next().ok_or("no workflow")?;
        assert!(
            text.contains(&format!(
                "uses: {}@{}  # {}",
                pin.action, pin.commit, pin.tag
            )),
            "{}\n{text}",
            pin.action
        );
    }
    Ok(())
}
