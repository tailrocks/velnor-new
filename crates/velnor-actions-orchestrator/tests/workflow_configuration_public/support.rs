// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write as _;

use super::workflow_configuration_fixture;

pub(super) fn config_with_timeouts(
    plan_timeout: u16,
    crate_timeout: Option<u16>,
) -> Result<String, std::fmt::Error> {
    let mut config = workflow_configuration_fixture::config_with_verification_branch(None);
    writeln!(config, "plan_timeout_minutes = {plan_timeout}")?;
    if let Some(crate_timeout) = crate_timeout {
        writeln!(config, "crate_timeout_minutes = {crate_timeout}")?;
    }
    Ok(config)
}

pub(super) fn rendered_job_block<'a>(
    workflow: &'a str,
    job_id: &str,
) -> Result<&'a str, Box<dyn Error>> {
    let marker = format!("  {job_id}:\n");
    let start = workflow
        .find(&marker)
        .ok_or_else(|| format!("rendered job {job_id} is missing"))?;
    let tail = &workflow[start..];
    let mut end = tail.len();
    let mut offset = 0;
    for (index, line) in tail.split_inclusive('\n').enumerate() {
        if index > 0
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
        {
            end = offset;
            break;
        }
        offset += line.len();
    }
    Ok(&tail[..end])
}
