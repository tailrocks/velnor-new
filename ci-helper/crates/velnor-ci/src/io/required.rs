// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::VerdictSpec;
use velnor_rust::{Verdict, decide};

use super::{EVENT_VARIABLE, SUMMARY_VARIABLE, append_to};
use crate::cli::GateArgs;
use crate::error::CiError;
use crate::required::parse_needs;

const NEEDS_VARIABLE: &str = "NEEDS_JSON";

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
    let spec = VerdictSpec::parse(&args.plan, &args.jobs, &args.tests).map_err(CiError::new)?;
    let verdict = judge(&spec);
    let table = verdict.table();
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
