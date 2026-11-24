// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value as Json;
use velnor_test_support::gh_expr::{Value, evaluate};

use crate::budget::budgeted;
use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::pr_ci_sim::{context_in_mode, other, simulate, strings};

#[test]
fn a_pin_that_reads_the_step_names_the_selection_in_a_step_and_keeps_the_job_name_static()
-> TestResult {
    let files = budgeted("0.1.116", "[budget]\nmain_wall_seconds = 600\n")?;
    let tree = crate::yaml_tree::document(&workflow(&files)?)?;
    let budget = tree
        .pointer("/jobs/rust-budget")
        .ok_or("the budget job is missing")?;
    assert_eq!(
        budget.get("name"),
        Some(&serde_json::json!("Speed budget")),
        "GitHub does not evaluate the name of a skipped job, so the name has no expression"
    );
    assert_eq!(
        strings(budget.get("needs")),
        ["required", "rust"],
        "the step reads the plan outputs, so the job needs the job that has them"
    );
    let step = budget
        .get("steps")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .find(|step| {
            step.get("name")
                .and_then(Json::as_str)
                .is_some_and(|name| name.contains("Check the speed budget"))
        })
        .ok_or("the budget step is missing")?;
    let name = step
        .get("name")
        .and_then(Json::as_str)
        .ok_or("the budget step has no name")?;
    let selection_env = step
        .pointer("/env/VELNOR_BUDGET_SELECTION")
        .and_then(Json::as_str)
        .ok_or("the command is not told the selection of the run")?;
    let event = other("push", "refs/heads/main");
    let results = simulate(&tree, &event, &[])?;
    let needs = strings(budget.get("needs"));
    for (mode, selection) in [("all", "full"), ("some", "affected"), ("none", "none")] {
        let shown = evaluate(name, &context_in_mode(&event, &results, &needs, mode))?;
        assert_eq!(
            shown,
            Value::Str(format!("Check the speed budget ({selection})")),
            "the step name when the plan selected {mode}\n{name}"
        );
        let told = evaluate(
            selection_env,
            &context_in_mode(&event, &results, &needs, mode),
        )?;
        assert_eq!(
            told,
            Value::Str(selection.to_owned()),
            "the selection the command is told when the plan selected {mode}\n{selection_env}"
        );
    }
    Ok(())
}
