// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, RootSlug, StateScope, StateTrust, StateUnit, outer_entry_bound,
    state_snapshot_output,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_LIST_LIMIT, STATE_SEAL_LIST_TAB, STATE_SEAL_PREDICATE_TYPE,
    STATE_SEAL_SELECT_LIST_LIMIT, STATE_SEAL_SELECT_PARALLELISM, STATE_SEAL_UNIT_TIMEOUT_ENV,
    STATE_SEAL_UNIT_TIMEOUT_SECONDS, STATE_SEAL_VERIFY_LIMIT, STATE_SEAL_VERIFY_TIMEOUT_ENV,
    STATE_SEAL_VERIFY_TIMEOUT_SECONDS, STATE_SEAL_WORKFLOW_PATH, state_attested_output,
    state_compat_output, state_produce_output,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use super::cache_keys::DEFAULT_REF_ENV;
use crate::RenderError;
use crate::cache_prune::state_prune_prefix;

const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const TOKEN_EXPR: &str = "${{ github.token }}";
const STEP_NAME: &str = "Select attested state";
const OUTPUT_DELIMITER: &str = "VELNOR_STATE_ATTESTED_EOF";
const CACHE_KEYS_OUTPUTS: &str = "steps.cache-keys.outputs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealRoot {
    pub unit: StateUnit,
    pub slug: RootSlug,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateSealPlan {
    pub roots: Vec<SealRoot>,
}

impl SealRoot {
    fn prefix(&self) -> String {
        StateScope {
            trust: StateTrust::Trusted,
            unit: self.unit,
            root: self.slug.clone(),
        }
        .prefix()
    }
}

const RUN_GH: &str = r#"run_gh() {
  if command -v timeout >/dev/null 2>&1; then timeout "$limit" gh "$@"; else gh "$@"; fi
}
"#;

const SELECT_HEAD: &str = r#"set -eu
case "$RUNNER_TEMP" in /*) ;; *) printf '%s\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac
d="$RUNNER_TEMP/velnor/state-select"
rm -rf "$d"
mkdir -p "$d"
ref="${VELNOR_DEFAULT_REF:-}"
repo="${GITHUB_REPOSITORY:-}"
signer="$repo/@SIGNER_PATH@"
limit="${@TIMEOUT_ENV@:-@TIMEOUT@}"
unit_limit="${@UNIT_TIMEOUT_ENV@:-@UNIT_TIMEOUT@}"
shape='^s256-[0-9a-f]{64}-s256-[0-9a-f]{64}-s256-[0-9a-f]{64}$'
export d ref repo signer limit shape
@RUN_GH@worker='@WORKER@'
unit() {
  mkdir "$d/u-$2"
  : > "$d/u-$2/out"
  printf '%s %s\n' "$1" "$2" >> "$d/units"
}
select_all() {
  [ -n "$ref" ] && [ -n "$repo" ] && [ -n "$signer" ] || return 0
  command -v gh >/dev/null 2>&1 || return 0
  run_gh cache list -R "$repo" --key '@STATE_PREFIX@' --ref "$ref" --sort created_at --order desc --limit @SELECT_LIMIT@ --json key,createdAt,id,sizeInBytes --jq '.[] | [.createdAt, .key, (.id | tostring), (.sizeInBytes | tostring)] | @tsv' > "$d/list" 2>/dev/null || return 0
  awk -v m=@SELECT_LIMIT@ 'END { exit (NR >= m) }' "$d/list" || return 0
  LC_ALL=C sort -r "$d/list" > "$d/sorted" || return 0
  if command -v timeout >/dev/null 2>&1; then xargs -P @PARALLELISM@ -n 2 timeout "$unit_limit" sh -c "$worker" sh < "$d/units" || :; else xargs -P @PARALLELISM@ -n 2 sh -c "$worker" sh < "$d/units" || :; fi
}
emit() {
  { printf '%s\n' "$1<<@DELIMITER@"; cat "$d/u-$2/out"; printf '%s\n' @DELIMITER@; } >> "$GITHUB_OUTPUT"
}
"#;

const WORKER: &str = r#"set -eu
@RUN_GH@w="$d/u-$2"
: > "$w/part"
m=
v=
while IFS="@TAB@" read -r created key id size; do
  case "$key" in "$1"?*) ;; *) continue ;; esac
  printf "%s\n" "${key#"$1"}" | grep -Eq "$shape" || continue
  m="${m}x"
  [ "${#m}" -le @LIST_LIMIT@ ] && [ "${#v}" -lt @VERIFY_LIMIT@ ] || break
  case "$id" in ""|*[!0-9]*) continue ;; esac
  case "$size" in ""|*[!0-9]*) continue ;; esac
  [ "$size" -le @OUTER_BOUND@ ] || continue
  printf "%s %s %s\n" "$key" "$id" "$size" > "$w/subject"
  if run_gh attestation verify "$w/subject" -R "$repo" --signer-workflow "$signer" --source-ref "$ref" --predicate-type "@PREDICATE@" >/dev/null 2>&1; then printf "%s\n" "$key" >> "$w/part"; v="${v}x"; fi
done < "$d/sorted"
mv -f "$w/part" "$w/out"
"#;

const DECIDE: &str = r#"decide() {
  want=false
  if [ -n "$4" ] && [ -n "$5" ]; then
    want=true
    prefix="$3$4-$5-"
    while IFS= read -r cand || [ -n "$cand" ]; do
      case "$cand" in "$prefix"?*) want=false ;; esac
    done < "$d/u-$2/out"
  fi
  printf '%s\n' "$1=$want" >> "$GITHUB_OUTPUT"
}
"#;

fn worker(outer_bound: u64) -> Result<String, RenderError> {
    let script = WORKER
        .replace("@RUN_GH@", RUN_GH)
        .replace("@OUTER_BOUND@", &outer_bound.to_string())
        .replace("@TAB@", STATE_SEAL_LIST_TAB)
        .replace("@LIST_LIMIT@", &STATE_SEAL_LIST_LIMIT.to_string())
        .replace("@VERIFY_LIMIT@", &STATE_SEAL_VERIFY_LIMIT.to_string())
        .replace("@PREDICATE@", STATE_SEAL_PREDICATE_TYPE);
    if script.contains('\'') {
        return Err(RenderError::InvalidWorkflow(
            "state_select:worker_single_quote".to_owned(),
        ));
    }
    Ok(crate::state_script::one_line(&script))
}

fn head(outer_bound: u64) -> Result<String, RenderError> {
    Ok(SELECT_HEAD
        .replace("@RUN_GH@", RUN_GH)
        .replace("@WORKER@", &worker(outer_bound)?)
        .replace("@TIMEOUT_ENV@", STATE_SEAL_VERIFY_TIMEOUT_ENV)
        .replace("@TIMEOUT@", &STATE_SEAL_VERIFY_TIMEOUT_SECONDS.to_string())
        .replace("@UNIT_TIMEOUT_ENV@", STATE_SEAL_UNIT_TIMEOUT_ENV)
        .replace(
            "@UNIT_TIMEOUT@",
            &STATE_SEAL_UNIT_TIMEOUT_SECONDS.to_string(),
        )
        .replace("@STATE_PREFIX@", &state_prune_prefix())
        .replace("@SELECT_LIMIT@", &STATE_SEAL_SELECT_LIST_LIMIT.to_string())
        .replace("@PARALLELISM@", &STATE_SEAL_SELECT_PARALLELISM.to_string())
        .replace("@DELIMITER@", OUTPUT_DELIMITER)
        .replace("@SIGNER_PATH@", STATE_SEAL_WORKFLOW_PATH))
}

pub fn state_select_step(roots: &[SealRoot]) -> Result<Step, RenderError> {
    let outer_bound = outer_entry_bound(CacheFamily::MbxBundle.spec().ceiling.stored())
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("state_select:outer_bound_unbounded".to_owned())
        })?;
    let mut sections = vec![head(outer_bound)?, DECIDE.to_owned()];
    let mut env = BTreeMap::from([
        (DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned()),
        ("GH_TOKEN".to_owned(), TOKEN_EXPR.to_owned()),
        (
            STATE_SEAL_VERIFY_TIMEOUT_ENV.to_owned(),
            STATE_SEAL_VERIFY_TIMEOUT_SECONDS.to_string(),
        ),
        (
            STATE_SEAL_UNIT_TIMEOUT_ENV.to_owned(),
            STATE_SEAL_UNIT_TIMEOUT_SECONDS.to_string(),
        ),
    ]);
    let mut decisions = Vec::with_capacity(roots.len());
    for (ordinal, root) in roots.iter().enumerate() {
        let compat_env = format!("VELNOR_STATE_COMPAT_R{ordinal}");
        let snapshot_env = format!("VELNOR_STATE_SNAPSHOT_R{ordinal}");
        env.insert(
            compat_env.clone(),
            format!(
                "${{{{ {CACHE_KEYS_OUTPUTS}.{} }}}}",
                state_compat_output(root.slug.as_str())
            ),
        );
        env.insert(
            snapshot_env.clone(),
            format!(
                "${{{{ {CACHE_KEYS_OUTPUTS}.{} }}}}",
                state_snapshot_output(&root.slug)
            ),
        );
        sections.push(format!("unit '{}' r{ordinal}\n", root.prefix()));
        decisions.push(format!(
            "emit {} r{ordinal}\ndecide {} r{ordinal} '{}' \"${{{compat_env}-}}\" \"${{{snapshot_env}-}}\"\n",
            state_attested_output(root.unit.as_str(), root.slug.as_str()),
            state_produce_output(root.unit.as_str(), root.slug.as_str()),
            root.prefix(),
        ));
    }
    sections.push("select_all\n".to_owned());
    sections.extend(decisions);
    let script = sections.concat();
    let mut step = crate::steps::ambient_shell_step(
        STEP_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script),
        ],
        env,
    )?;
    step.id = Some(StepId::StateSelect);
    step.role = Some(StepRole::StateSelect);
    Ok(step)
}
