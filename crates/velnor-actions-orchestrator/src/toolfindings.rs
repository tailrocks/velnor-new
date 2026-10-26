// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::Finding;
use velnor_actions_rust::MISSING_RECOMMENDED_INPUT;

use crate::toolcheck::{ToolInputCheck, ToolParse};

pub const CONFLICTING_TOOL_VALUES: &str = "conflicting_tool_values";
pub const UNSUPPORTED_TOOL_VALUE: &str = "unsupported_tool_value";

#[must_use]
pub fn tool_check_lines(checks: &[ToolInputCheck]) -> Vec<String> {
    let mut lines = Vec::new();
    for check in checks {
        let problem = match &check.parse {
            ToolParse::Invalid { problem } => Some(problem.as_str()),
            ToolParse::Unreadable => Some("unreadable"),
            ToolParse::Missing | ToolParse::Valid => None,
        };
        if let Some(problem) = problem {
            lines.push(format!(
                "{}: {}: {problem}; fix it manually, Velnor continues with its pinned tools",
                velnor_actions_rust::TOOLING_INPUT_INVALID,
                check.path
            ));
        }
        if check.path == "rust-toolchain.toml"
            && check.parse == ToolParse::Valid
            && !check.values.contains_key("channel")
        {
            lines.push(format!(
                "{MISSING_RECOMMENDED_INPUT}: {}: no [toolchain] channel; \
                 add an exact channel pin manually",
                check.path
            ));
        }
    }
    lines.sort();
    lines
}

#[must_use]
pub fn finding_line(finding: &Finding) -> String {
    let observed = finding.observed.as_deref().unwrap_or("present");
    let guidance = finding
        .action
        .as_deref()
        .or(finding.recommended.as_deref())
        .unwrap_or("review manually");
    format!("{}: {}: {observed}; {guidance}", finding.code, finding.path)
}

#[must_use]
pub fn tool_conflicts(checks: &[ToolInputCheck]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let channel = value_for(checks, "rust-toolchain.toml", "channel");
    let mise_rust = value_for(checks, "mise.toml", "tools.rust");
    if let (Some(left), Some(right)) = (channel, mise_rust)
        && comparable(left, right)
        && left != right
    {
        findings.push(Finding {
            code: CONFLICTING_TOOL_VALUES.to_owned(),
            path: "rust-toolchain.toml".to_owned(),
            observed: Some(format!("channel {left} vs mise.toml tools.rust {right}")),
            recommended: None,
            action: Some(
                "align both pins manually to one exact Rust version; \
                 Velnor keeps its own exact pin"
                    .to_owned(),
            ),
            reason: "generated steps use Velnor pins, but divergent project \
                 pins confuse editors and non-Velnor runs"
                .to_owned(),
        });
    }
    for (path, key) in [
        ("rust-toolchain.toml", "channel"),
        ("mise.toml", "tools.rust"),
    ] {
        if let Some(value) = value_for(checks, path, key)
            && !is_version_like(value)
            && !is_channel_like(value)
        {
            findings.push(Finding {
                code: UNSUPPORTED_TOOL_VALUE.to_owned(),
                path: path.to_owned(),
                observed: Some(format!("{key} {value}")),
                recommended: None,
                action: Some(
                    "set an exact version or stable, beta, or nightly manually".to_owned(),
                ),
                reason: "unrecognized Rust selectors cannot guide manual alignment".to_owned(),
            });
        }
    }
    findings.sort_by(|left, right| left.code.cmp(&right.code).then(left.path.cmp(&right.path)));
    findings
}

fn value_for<'a>(checks: &'a [ToolInputCheck], path: &str, key: &str) -> Option<&'a str> {
    checks.iter().find_map(|check| {
        if check.path == path {
            check.values.get(key).map(String::as_str)
        } else {
            None
        }
    })
}

fn comparable(left: &str, right: &str) -> bool {
    (is_version_like(left) && is_version_like(right))
        || (is_channel_like(left) && is_channel_like(right))
}

fn is_version_like(value: &str) -> bool {
    value
        .strip_prefix('v')
        .unwrap_or(value)
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_digit())
}

fn is_channel_like(value: &str) -> bool {
    matches!(value, "stable" | "beta" | "nightly") || value.starts_with("nightly-")
}
