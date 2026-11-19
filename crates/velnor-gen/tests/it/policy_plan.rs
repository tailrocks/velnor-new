// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::golden::{load, samples};
use crate::pipeline::workflow;

fn job<'a>(text: &'a str, id: &str) -> Result<&'a str, String> {
    let header = format!("\n  {id}:\n");
    let start = text.find(&header).ok_or(format!("no job {id}"))? + header.len();
    let rest = &text[start..];
    Ok(rest
        .match_indices("\n  ")
        .find(|(at, _)| rest[at + 3..].chars().next().is_some_and(|c| c != ' '))
        .map_or(rest, |(at, _)| &rest[..at]))
}

#[test]
fn one_root_policy_workflow_plans_once_and_starts_the_policy_job_only_for_a_policy_tool()
-> TestResult {
    let text = workflow(&load(&samples().join("policy-lane"))?)?;
    assert_eq!(text.matches("      - id: plan\n").count(), 1, "{text}");
    let policy = job(&text, "policy")?;
    assert!(!policy.contains("velnor-ci plan"), "{policy}");
    assert!(
        policy.contains(
            "    if: '${{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (!cancelled() && (needs.rust.outputs.rust_deny == ''run'' || needs.rust.outputs.rust_machete == ''run'')) }}'\n"
        ),
        "{policy}"
    );
    assert!(policy.contains("    needs:\n      - rust\n"), "{policy}");
    let rust = job(&text, "rust")?;
    assert!(
        rust.contains("      rust_deny: '${{ steps.plan.outputs.rust_deny }}'\n"),
        "{rust}"
    );
    assert!(
        rust.contains("      rust_machete: '${{ steps.plan.outputs.rust_machete }}'\n"),
        "{rust}"
    );
    let required = job(&text, "required")?;
    assert!(
        required.contains("      - rust\n      - policy\n"),
        "{required}"
    );
    Ok(())
}
