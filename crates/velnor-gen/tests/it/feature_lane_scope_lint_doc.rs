// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_test_support::gh_expr::{Value, evaluate};

use crate::facts::TestResult;
use crate::feature_lanes::{lane_view, steps_named};
use crate::pipeline::workflow;

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\nlint = true\ndoc = true\n";
const STEPS: [&str; 2] = [
    "Lint (features: local-mode)",
    "Documentation (features: local-mode)",
];

fn plan(mode: &'static str, packages: &'static str) -> impl Fn(&str) -> Result<Value, String> {
    move |name: &str| {
        let text = |value: &str| Ok(Value::Str(value.to_owned()));
        match name {
            "github.event_name" => text("pull_request"),
            "github.ref" => text("refs/pull/7/merge"),
            "github.event.pull_request.state" => text("open"),
            "needs.rust-plan.outputs.skip_push" => text("false"),
            "steps.replan.outcome" => text("skipped"),
            _ if name.ends_with("_mode") => text(mode),
            _ if name.ends_with("_docs") => text("run"),
            _ if name.ends_with("_package_args") => text(packages),
            _ if name.ends_with(".outcome") || name.ends_with(".result") => text("success"),
            _ => Ok(Value::Null),
        }
    }
}

fn opens(mode: &'static str, packages: &'static str) -> Result<Vec<bool>, String> {
    let repo = lane_view("0.1.108", LANE).map_err(|error| error.to_string())?;
    let document = workflow(&repo).map_err(|error| error.to_string())?;
    let jobs =
        crate::yaml_tree::document(&document).map_err(|error| error.to_string())?["jobs"].clone();
    let truthy = |condition: &serde_json::Value| -> Result<bool, String> {
        match condition.as_str() {
            Some(text) => Ok(evaluate(text, &plan(mode, packages))?.truthy()),
            None => Ok(true),
        }
    };
    STEPS
        .iter()
        .map(|name| {
            let found = steps_named(&jobs, name);
            let (job, _, step) = found.first().ok_or(format!("no step {name}"))?;
            Ok(truthy(&jobs[job]["if"])? && truthy(&step["if"])?)
        })
        .collect()
}

#[test]
fn a_change_to_a_dependent_only_opens_the_lane_lint_and_doc_steps() -> TestResult {
    assert_eq!(opens("some", "-p dependent")?, [true, true]);
    Ok(())
}

#[test]
fn a_plan_that_selects_no_member_skips_the_lane_lint_and_doc_steps() -> TestResult {
    assert_eq!(opens("none", "")?, [false, false]);
    Ok(())
}
