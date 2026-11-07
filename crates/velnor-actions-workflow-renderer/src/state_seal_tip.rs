// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_TIP_CONDITION, STATE_SEAL_TIP_OUTPUT,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use crate::RenderError;
use crate::steps::ambient_shell_step;

const TOKEN_EXPR: &str = "${{ github.token }}";
const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";

pub fn state_seal_tip_step() -> Result<Step, RenderError> {
    let mut step = tip_step(&format!(
        "printf '%s\\n' '{STATE_SEAL_TIP_OUTPUT}=false' >> \"$GITHUB_OUTPUT\"; exit 0"
    ))?;
    step.condition = Some(STATE_SEAL_TIP_CONDITION.to_owned());
    Ok(step)
}

pub fn stale_tip_stops_job_step() -> Result<Step, RenderError> {
    tip_step("exit 1")
}

fn tip_step(skip: &str) -> Result<Step, RenderError> {
    let script = format!(
        r#"set -eu
case "$RUNNER_TEMP" in /*) ;; *) printf '%s\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac
d="$RUNNER_TEMP/velnor/state-tip"
rm -rf "$d"
mkdir -p "$d"
refuse() {{ printf '%s\n' "::warning::cache_unavailable: state seal tip check refused ($1)"; {skip}; }}
head="${{GITHUB_SHA-}}"
ref="${{{DEFAULT_REF_ENV}-}}"
repo="${{GITHUB_REPOSITORY-}}"
[ -n "$head" ] && [ -n "$ref" ] && [ -n "$repo" ] || refuse input_missing
command -v gh >/dev/null 2>&1 || refuse gh_missing
gh api "repos/$repo/git/ref/${{ref#refs/}}" --jq .object.sha > "$d/tip" 2> /dev/null || refuse tip_unreadable
tip=
read -r tip < "$d/tip" || :
case "$tip" in ''|*[!0-9a-f]*) refuse tip_malformed ;; esac
if [ "$tip" = "$head" ]; then
  printf '%s\n' '{STATE_SEAL_TIP_OUTPUT}=true' >> "$GITHUB_OUTPUT"
  printf 'velnor: state seal head %s is the tip of %s\n' "$head" "$ref"
else
  printf 'velnor: state seal skipped: head %s is behind %s at %s; nothing saved or attested\n' "$head" "$ref" "$tip"
  {skip}
fi
"#,
    );
    let mut step = ambient_shell_step(
        "Check state tip",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "set -eu; {}; printf 'VELNOR_VERIFIED_GH_DATA_DIR=%s\\n' \"$VELNOR_VERIFIED_GH_DATA_DIR\" >> \"$GITHUB_ENV\"; gh() {{ {} \"$@\"; }}; {}",
                crate::verified_gh_shell::bootstrap(),
                crate::verified_gh_shell::command(),
                crate::state_script::one_line(&script)
            ),
        ],
        BTreeMap::from([
            (DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned()),
            ("GH_TOKEN".to_owned(), TOKEN_EXPR.to_owned()),
        ]),
    )?;
    step.id = Some(StepId::StateSealTip);
    step.role = Some(StepRole::StateSealTip);
    Ok(step)
}
