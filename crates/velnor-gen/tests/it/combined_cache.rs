// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, check_rules};

fn uses(text: &str) -> RenderedStep {
    RenderedStep {
        name: "step".to_owned(),
        uses: Some(text.to_owned()),
        ..RenderedStep::default()
    }
}

fn run(text: &str) -> RenderedStep {
    RenderedStep {
        name: "step".to_owned(),
        run: Some(text.to_owned()),
        ..RenderedStep::default()
    }
}

#[test]
fn the_combined_cache_action_is_refused_before_a_later_step() {
    let cache = uses("actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9");
    let job = |steps: Vec<RenderedStep>| RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "produce".to_owned(),
            runs_on: "ubuntu-latest".to_owned(),
            timeout_minutes: 10,
            steps,
            ..RenderedJob::default()
        }],
    };
    let rules = |workflow: RenderedWorkflow| match check_rules(&workflow) {
        Ok(()) => Vec::new(),
        Err(found) => found
            .into_iter()
            .map(|violation| (violation.step, violation.rule))
            .collect(),
    };
    assert_eq!(
        rules(job(vec![cache.clone(), run("true")])),
        [(0, Rule::CombinedCacheNotLastStep)]
    );
    assert_eq!(rules(job(vec![run("true"), cache])), []);
}
