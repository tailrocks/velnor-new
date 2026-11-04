// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;
use crate::steps::shell_step;

pub const MBX_STATS_JOB_ENV: &str = "VELNOR_MBX_STATS_JOB";
pub const MBX_STATS_WORKSPACE_ENV: &str = "VELNOR_MBX_STATS_WORKSPACE";
pub const MBX_STATS_CONDITION: &str = "always()";
const STEP_NAME: &str = "MBX stats";

const PROGRAM: &str = r#"[inputs | fromjson? | select(type == "object" and .type == "action")] as $a | ($members | split(" ") | map(select(length > 0) | {key: ., value: true}) | from_entries) as $m | def benign: .outcome.kind == "bypass" and (.outcome.reason == "compiler-query" or .outcome.reason == "standard-input"); def n($c; $k): [$a[] | select((benign | not) and .outcome.kind == $k and ((if $m[.crate_name // ""] then "workspace" else "registry" end) == $c))] | length; def group($c): "\($c) hit=\(n($c; "hit")) miss=\(n($c; "miss")) not_looked_up=\(n($c; "unconsulted")) bypassed=\(n($c; "bypass"))"; "mbx-stats job=\($job) \(group("registry")) \(group("workspace")) bypassed_benign=\([$a[] | select(benign)] | length)""#;

fn script() -> String {
    format!(
        r#"set -eu
job="${{{MBX_STATS_JOB_ENV}-}}"
members="${{{MBX_STATS_WORKSPACE_ENV}-}}"
out="${{RUNNER_TEMP:?}}/velnor-mbx-stats.line"
set -- "${{MBX_CACHE_DIR-}}"/actions/sessions/v1/*.jsonl
if [ -z "${{MBX_CACHE_DIR-}}" ] || [ ! -f "$1" ]; then printf 'mbx-stats job=%s sessions=0\n' "$job" > "$out"; else jq -r -n -R --arg job "$job" --arg members "$members" '{PROGRAM}' "$@" > "$out"; fi
read -r line < "$out"
printf '%s\n' "$line"
if [ -n "${{GITHUB_STEP_SUMMARY-}}" ]; then printf '%s\n' "$line" >> "$GITHUB_STEP_SUMMARY"; fi
"#
    )
}

pub fn mbx_stats_step(job_id: &str, workspace_crates: &[String]) -> Result<Step, RenderError> {
    let mut step = shell_step(
        STEP_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script()),
        ],
        BTreeMap::from([
            (MBX_STATS_JOB_ENV.to_owned(), job_id.to_owned()),
            (
                MBX_STATS_WORKSPACE_ENV.to_owned(),
                workspace_crates.join(" "),
            ),
        ]),
    )?;
    step.role = Some(StepRole::MbxStats);
    step.condition = Some(MBX_STATS_CONDITION.to_owned());
    Ok(step)
}
