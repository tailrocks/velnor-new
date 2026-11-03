// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    OUTER_ENTRY_HEADER_BYTES, TAR_BLOCK_BYTES, TAR_END_OF_ARCHIVE_BYTES, TAR_RECORD_BYTES,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_ADMITTED_OUTPUT, STATE_SEAL_ARTIFACT_ENV, STATE_SEAL_BUDGET_BYTES,
    STATE_SEAL_KEY_ENV, STATE_SEAL_LIST_LIMIT, STATE_SEAL_PENDING_BYTES_ENV,
    STATE_SEAL_PENDING_ENV, STATE_SEAL_PREDICATE_TYPE, STATE_SEAL_RETIRE_CONDITION,
    STATE_SEAL_SCOPE_ENV, STATE_SEAL_SUBJECT_REL,
};
use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;
use crate::steps::ambient_shell_step;

const TOKEN_EXPR: &str = "${{ github.token }}";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const PAYLOAD: &str = "$RUNNER_TEMP/velnor-gc1/transport/state/payload.tar.zst";
const ZSTD_SMALL_INPUT_BYTES: u64 = 128 << 10;
const ZSTD_MARGIN_SHIFT_DIVISOR: u64 = 1 << 11;

const HEAD: &str = r#"set -eu
case "$RUNNER_TEMP" in /*) ;; *) printf '%s\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac
payload="@PAYLOAD@"
d="$RUNNER_TEMP/velnor/@DIR@"
rm -rf "$d"
mkdir -p "$d"
number() { case "$1" in ''|*[!0-9]*) return 1 ;; esac; }
bound_of() {
  awk -v n="$1" 'BEGIN { b = int((n + @BLOCK@ - 1) / @BLOCK@) * @BLOCK@; f = b + @FRAME@; t = int((f + @RECORD@ - 1) / @RECORD@) * @RECORD@; m = 0; if (t < @SMALL@) m = int((@SMALL@ - t) / @MARGIN@); printf "%.0f\n", t + int(t / 256) + m }' > "$d/bound" || return 1
  bound=; read -r bound < "$d/bound" || return 1
  number "$bound"
}
size=
if [ -L "$payload" ] || [ ! -f "$payload" ]; then size=; else wc -c < "$payload" > "$d/size" && read -r size < "$d/size" || size=; fi
"#;

const ADMIT: &str = r#"refuse() { printf '%s\n' "::warning::cache_unavailable: state save refused ($1)"; printf '%s\n' '@ADMITTED@=false' >> "$GITHUB_OUTPUT"; exit 0; }
number "$size" && [ "$size" -gt 0 ] || refuse payload_unreadable
bound_of "$size" || refuse bound_unavailable
gh api "repos/$GITHUB_REPOSITORY/actions/cache/usage" --jq .active_caches_size_in_bytes > "$d/usage" 2> /dev/null || refuse usage_unavailable
usage=
read -r usage < "$d/usage" || usage=
number "$usage" || refuse usage_unreadable
unit="${@PENDING_BYTES@:-0}"
number "$unit" || refuse pending_unreadable
printf '%s\n' "${@PENDING@-}" | awk -v unit="$unit" '{ for (i = 1; i <= NF; i++) if ($i == "true") n++ } END { printf "%.0f\n", n * unit }' > "$d/reserved" || refuse reserve_unreadable
reserved=; read -r reserved < "$d/reserved" || reserved=
number "$reserved" || refuse reserve_unreadable
awk -v u="$usage" -v b="$bound" -v r="$reserved" 'BEGIN { exit !(u + b + r <= @BUDGET@) }' || refuse "over_budget usage=$usage bound=$bound reserved=$reserved budget=@BUDGET@"
printf '%s\n' '@ADMITTED@=true' >> "$GITHUB_OUTPUT"
printf 'velnor: state save admitted usage=%s bound=%s reserved=%s budget=%s\n' "$usage" "$bound" "$reserved" @BUDGET@
"#;

const RETIRE: &str = r#"skip() { printf '%s\n' "::warning::cache_unavailable: state retire skipped ($1)"; exit 0; }
key="${@KEY_ENV@-}"
scope="${@SCOPE@-}"
ref="${@REF_ENV@-}"
repo="${GITHUB_REPOSITORY-}"
signer="${GITHUB_WORKFLOW_REF-}"
signer="${signer%%@*}"
[ -n "$key" ] && [ -n "$scope" ] && [ -n "$ref" ] && [ -n "$repo" ] && [ -n "$signer" ] || skip identity_missing
number "$size" && [ "$size" -gt 0 ] || skip payload_unreadable
bound_of "$size" || skip bound_unavailable
subject="$RUNNER_TEMP/@SUBJECT@"
if [ -L "$subject" ] || [ ! -f "$subject" ]; then skip subject_missing; fi
sk=; sid=; stored=
read -r sk sid stored < "$subject" || skip subject_unreadable
[ "$sk" = "$key" ] || skip subject_key_mismatch
number "$sid" && number "$stored" && [ "$stored" -gt 0 ] && [ "$stored" -le "$bound" ] || skip "size_out_of_bound stored=$stored bound=$bound"
gh attestation verify "$subject" -R "$repo" --signer-workflow "$signer" --source-ref "$ref" --predicate-type '@PREDICATE@' > /dev/null 2>&1 || skip attestation_unverified
shape='^s256-[0-9a-f]{64}-s256-[0-9a-f]{64}-s256-[0-9a-f]{64}$'
gh cache list --key "$scope" --ref "$ref" --limit @LIMIT@ --json id,key --jq '.[] | "\(.id) \(.key)"' > "$d/scope" 2> /dev/null || skip scope_list_failed
while read -r id k; do
  case "$k" in "$scope"?*) ;; *) continue ;; esac
  printf '%s\n' "${k#"$scope"}" | grep -Eq "$shape" || continue
  [ "$k" != "$key" ] || continue
  if gh cache delete "$id" > /dev/null 2>&1; then printf 'velnor: retired superseded state entry %s\n' "$k"; else printf '%s\n' "::warning::cache_unavailable: could not delete state entry $id"; fi
done < "$d/scope"
printf 'velnor: state snapshot published (%s bytes, sha256:%s)\n' "$size" "${key##*-s256-}"
artifact="${@ARTIFACT@-}"
if number "$artifact" && [ -n "$artifact" ]; then
  if gh api -X DELETE "repos/$repo/actions/artifacts/$artifact" > /dev/null 2>&1; then printf 'velnor: deleted handoff artifact %s\n' "$artifact"; else printf '%s\n' "::warning::cache_unavailable: could not delete handoff artifact $artifact"; fi
fi
"#;

fn head(dir: &str) -> String {
    HEAD.replace("@PAYLOAD@", PAYLOAD)
        .replace("@DIR@", dir)
        .replace("@BLOCK@", &TAR_BLOCK_BYTES.to_string())
        .replace(
            "@FRAME@",
            &(OUTER_ENTRY_HEADER_BYTES + TAR_END_OF_ARCHIVE_BYTES).to_string(),
        )
        .replace("@RECORD@", &TAR_RECORD_BYTES.to_string())
        .replace("@SMALL@", &ZSTD_SMALL_INPUT_BYTES.to_string())
        .replace("@MARGIN@", &ZSTD_MARGIN_SHIFT_DIVISOR.to_string())
}

fn gh_step(name: &str, script: &str, env: BTreeMap<String, String>) -> Result<Step, RenderError> {
    let mut env = env;
    env.insert("GH_TOKEN".to_owned(), TOKEN_EXPR.to_owned());
    ambient_shell_step(
        name,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(script),
        ],
        env,
    )
}

pub fn state_seal_admit_step(pending: &[String], pending_bytes: u64) -> Result<Step, RenderError> {
    let script = format!(
        "{}{}",
        head("state-admit"),
        ADMIT
            .replace("@ADMITTED@", STATE_SEAL_ADMITTED_OUTPUT)
            .replace("@PENDING_BYTES@", STATE_SEAL_PENDING_BYTES_ENV)
            .replace("@PENDING@", STATE_SEAL_PENDING_ENV)
            .replace("@BUDGET@", &STATE_SEAL_BUDGET_BYTES.to_string())
    );
    let mut step = gh_step(
        "Admit state save",
        &script,
        BTreeMap::from([
            (STATE_SEAL_PENDING_ENV.to_owned(), pending.join(" ")),
            (
                STATE_SEAL_PENDING_BYTES_ENV.to_owned(),
                pending_bytes.to_string(),
            ),
        ]),
    )?;
    step.id = Some(velnor_actions_contract::StepId::StateSealAdmit);
    step.role = Some(StepRole::StateSealAdmit);
    step.condition = Some(
        velnor_actions_contract::workflow::state_seal::STATE_SEAL_SUBJECT_CONDITION.to_owned(),
    );
    Ok(step)
}

pub fn state_seal_retire_step(scope: &str, artifact_expr: &str) -> Result<Step, RenderError> {
    let script = format!(
        "{}{}",
        head("state-retire"),
        RETIRE
            .replace("@KEY_ENV@", STATE_SEAL_KEY_ENV)
            .replace("@SCOPE@", STATE_SEAL_SCOPE_ENV)
            .replace("@REF_ENV@", DEFAULT_REF_ENV)
            .replace("@ARTIFACT@", STATE_SEAL_ARTIFACT_ENV)
            .replace("@LIMIT@", &STATE_SEAL_LIST_LIMIT.to_string())
            .replace("@SUBJECT@", STATE_SEAL_SUBJECT_REL)
            .replace("@PREDICATE@", STATE_SEAL_PREDICATE_TYPE)
    );
    let mut step = gh_step(
        "Retire superseded state",
        &script,
        BTreeMap::from([
            (
                STATE_SEAL_KEY_ENV.to_owned(),
                velnor_actions_contract::workflow::state_seal::STATE_SEAL_KEY_EXPR.to_owned(),
            ),
            (STATE_SEAL_SCOPE_ENV.to_owned(), scope.to_owned()),
            (DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned()),
            (STATE_SEAL_ARTIFACT_ENV.to_owned(), artifact_expr.to_owned()),
        ]),
    )?;
    step.role = Some(StepRole::StateSealRetire);
    step.condition = Some(STATE_SEAL_RETIRE_CONDITION.to_owned());
    Ok(step)
}
