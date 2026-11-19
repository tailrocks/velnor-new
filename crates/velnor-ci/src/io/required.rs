// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BUDGET_JOB, BUDGET_MAIN_WALL, BUDGET_PR_WALL, BUDGET_RUNNER, JobId, RUN_RUNNER, RUN_WALL,
    VerdictSpec,
};
use velnor_rust::{Verdict, decide};

use super::{EVENT_VARIABLE, SUMMARY_VARIABLE, append_to};
use crate::cli::GateArgs;
use crate::error::CiError;
use crate::required::parse_needs;

const NEEDS_VARIABLE: &str = "NEEDS_JSON";
const PULL_REQUEST: &str = "pull_request";

fn judge(spec: &VerdictSpec) -> Verdict {
    let Some(text) = std::env::var(NEEDS_VARIABLE)
        .ok()
        .filter(|text| !text.is_empty())
    else {
        return Verdict::failed(format!("{NEEDS_VARIABLE} is not set"));
    };
    match parse_needs(&text) {
        Ok(needs) => decide(spec, &needs, std::env::var(EVENT_VARIABLE).ok().as_deref()),
        Err(error) => Verdict::failed(error.to_string()),
    }
}

pub(super) fn run_gate(args: &GateArgs) -> Result<String, CiError> {
    let text = std::env::var(&args.spec_env)
        .ok()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| CiError::new(format!("{} is not set", args.spec_env)))?;
    let spec = VerdictSpec::from_text(&text).map_err(CiError::new)?;
    let verdict = judge(&spec);
    let mut table = verdict.table();
    table.push_str(&budget_table());
    print!("{table}");
    append_to(SUMMARY_VARIABLE, &table)?;
    if verdict.passed {
        Ok(String::new())
    } else {
        Err(CiError::new(format!(
            "the required check fails: {}",
            verdict.problems.join("; ")
        )))
    }
}

fn budget_table() -> String {
    let Some(text) = std::env::var(NEEDS_VARIABLE)
        .ok()
        .filter(|text| !text.is_empty())
    else {
        return String::new();
    };
    let Ok(needs) = parse_needs(&text) else {
        return String::new();
    };
    let Some(need) = needs.get(&JobId::from_static(BUDGET_JOB)) else {
        return String::new();
    };
    let pull_request = std::env::var(EVENT_VARIABLE).is_ok_and(|event| event == PULL_REQUEST);
    let wall_budget = need.outputs.get(if pull_request {
        BUDGET_PR_WALL
    } else {
        BUDGET_MAIN_WALL
    });
    let rows = [
        ("Wall seconds", need.outputs.get(RUN_WALL), wall_budget),
        (
            "Runner seconds",
            need.outputs.get(RUN_RUNNER),
            need.outputs.get(BUDGET_RUNNER),
        ),
    ];
    if rows
        .iter()
        .all(|(_, current, budget)| current.is_none() && budget.is_none())
    {
        return String::new();
    }
    let mut lines = vec![
        "**Speed budget**".to_owned(),
        String::new(),
        "| Measure | This run so far | Budget |".to_owned(),
        "|---|---:|---:|".to_owned(),
    ];
    for (measure, current, budget) in rows {
        lines.push(format!(
            "| {measure} | {} | {} |",
            shown(current),
            shown(budget)
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn shown(value: Option<&String>) -> String {
    value.map_or_else(
        || "not measured".to_owned(),
        |value| match value.parse::<u64>() {
            Ok(0) => "off".to_owned(),
            Ok(seconds) => format!("{seconds}s"),
            Err(_) => value.clone(),
        },
    )
}
