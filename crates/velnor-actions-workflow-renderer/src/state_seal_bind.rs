// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_BIND_ATTEMPTS, STATE_SEAL_BIND_CONDITION, STATE_SEAL_BIND_PAUSE_SECONDS,
    STATE_SEAL_KEY_ENV, STATE_SEAL_KEY_EXPR, STATE_SEAL_LIST_FIELDS, STATE_SEAL_LIST_JQ,
    STATE_SEAL_LIST_LIMIT, STATE_SEAL_LIST_TAB, STATE_SEAL_SUBJECT_REL,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use super::{BOUND_OUTPUT, TOKEN_EXPR};
use crate::RenderError;
use crate::steps::ambient_shell_step;

pub fn state_seal_bind_step() -> Result<Step, RenderError> {
    let script = format!(
        r#"{head}subject="$RUNNER_TEMP/{rel}"
mkdir -p "${{subject%/*}}"
rm -f "$subject"
refuse() {{ printf '%s\n' "::warning::cache_unavailable: state seal bind refused ($1)"; printf '%s\n' '{bound}=false' >> "$GITHUB_OUTPUT"; exit 0; }}
key="${{{key_env}-}}"
ref="${{GITHUB_REF-}}"
repo="${{GITHUB_REPOSITORY-}}"
[ -n "$key" ] && [ -n "$ref" ] && [ -n "$repo" ] || refuse input_missing
case "$key" in *[[:space:]]*) refuse key_whitespace ;; esac
number "$size" && [ "$size" -gt 0 ] || refuse payload_unreadable
bound_of "$size" || refuse bound_unavailable
command -v gh >/dev/null 2>&1 || refuse gh_missing
tries=
while :; do
  gh cache list -R "$repo" --key "$key" --ref "$ref" --limit {list_limit} --json {fields} --jq '{jq}' > "$d/list" || refuse list_failed
  hits=
  id=
  stored=
  while IFS='{tab}' read -r k i s; do
    [ "$k" = "$key" ] || continue
    case "$i" in ''|*[!0-9]*) refuse id_malformed ;; esac
    case "$s" in ''|*[!0-9]*) refuse size_malformed ;; esac
    hits="${{hits}}x"
    id=$i
    stored=$s
  done < "$d/list"
  [ -z "$hits" ] || break
  tries="${{tries}}x"
  [ "${{#tries}}" -lt {attempts} ] || break
  sleep {pause}
done
[ "$hits" = x ] || refuse "entries=${{#hits}}"
[ "$stored" -le "$bound" ] || refuse "size_out_of_bound stored=$stored bound=$bound"
printf '%s %s %s\n' "$key" "$id" "$stored" > "$subject"
printf '%s\n' '{bound}=true' >> "$GITHUB_OUTPUT"
printf 'velnor: state seal subject %s %s %s\n' "$key" "$id" "$stored"
"#,
        head = crate::state_seal::save::head("state-bind"),
        rel = STATE_SEAL_SUBJECT_REL,
        bound = BOUND_OUTPUT,
        key_env = STATE_SEAL_KEY_ENV,
        list_limit = STATE_SEAL_LIST_LIMIT,
        fields = STATE_SEAL_LIST_FIELDS,
        jq = STATE_SEAL_LIST_JQ,
        tab = STATE_SEAL_LIST_TAB,
        attempts = STATE_SEAL_BIND_ATTEMPTS,
        pause = STATE_SEAL_BIND_PAUSE_SECONDS,
    );
    let mut step = ambient_shell_step(
        "Bind seal subject",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script),
        ],
        BTreeMap::from([
            (
                STATE_SEAL_KEY_ENV.to_owned(),
                STATE_SEAL_KEY_EXPR.to_owned(),
            ),
            ("GH_TOKEN".to_owned(), TOKEN_EXPR.to_owned()),
        ]),
    )?;
    step.id = Some(StepId::StateSealBind);
    step.role = Some(StepRole::StateSealBind);
    step.condition = Some(STATE_SEAL_BIND_CONDITION.to_owned());
    Ok(step)
}
