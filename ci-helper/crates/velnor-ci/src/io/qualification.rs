// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::JobId;

use super::{SUMMARY_VARIABLE, append_to};
use crate::cli::QualificationCompareArgs;
use crate::error::CiError;
use crate::qualification::compare;
use crate::required::parse_needs;

const NEEDS_VARIABLE: &str = "NEEDS_JSON";

fn lane_ids(args: &QualificationCompareArgs) -> Result<Vec<JobId>, CiError> {
    args.lane
        .iter()
        .map(|lane| JobId::new(lane.as_str()).map_err(|e| CiError::new(e.to_string())))
        .collect()
}

pub(super) fn run_compare(args: &QualificationCompareArgs) -> Result<String, CiError> {
    let lanes = lane_ids(args)?;
    let text = std::env::var(NEEDS_VARIABLE)
        .ok()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| CiError::new(format!("{NEEDS_VARIABLE} is not set")))?;
    let comparison = compare(&parse_needs(&text)?, &lanes);
    let table = comparison.table();
    print!("{table}");
    append_to(SUMMARY_VARIABLE, &table)?;
    if comparison.passed {
        Ok(String::new())
    } else {
        Err(CiError::new(format!(
            "the qualification compare fails: {}",
            comparison.problems.join("; ")
        )))
    }
}
