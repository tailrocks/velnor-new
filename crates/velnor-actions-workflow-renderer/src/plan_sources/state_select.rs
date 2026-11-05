// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    RootSlug, SliceId, StateScope, StateTrust, StateUnit, outer_entry_bound, state_snapshot_output,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_MATRIX_ATTESTED, STATE_MATRIX_COMPAT, STATE_MATRIX_PENDING, STATE_MATRIX_SLICE,
    STATE_MATRIX_SNAPSHOT, STATE_SEAL_LIST_LIMIT, STATE_SEAL_LIST_TAB, STATE_SEAL_MATRIX_OUTPUT,
    STATE_SEAL_PREDICATE_TYPE, STATE_SEAL_SELECT_LIST_LIMIT, STATE_SEAL_SELECT_PARALLELISM,
    STATE_SEAL_UNIT_TIMEOUT_ENV, STATE_SEAL_UNIT_TIMEOUT_SECONDS, STATE_SEAL_VERIFY_LIMIT,
    STATE_SEAL_VERIFY_TIMEOUT_ENV, STATE_SEAL_VERIFY_TIMEOUT_SECONDS, STATE_SEAL_WORKFLOW_PATH,
    state_attested_output, state_compat_output, state_producer_matrix_output,
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
pub struct SealUnit {
    pub slice: SliceId,
    pub root: RootSlug,
}

impl SealUnit {
    #[must_use]
    pub fn unit(&self) -> StateUnit {
        StateUnit::Slice(self.slice.clone())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateSealPlan {
    pub units: Vec<SealUnit>,
}

fn unit_prefix(unit: &StateUnit) -> String {
    StateScope {
        trust: StateTrust::Trusted,
        unit: unit.clone(),
    }
    .prefix()
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
  printf '%s %s %s\n' "$1" "$2" "$3" >> "$d/units"
}
select_all() {
  [ -n "$ref" ] && [ -n "$repo" ] && [ -n "$signer" ] || return 0
  command -v gh >/dev/null 2>&1 || return 0
  run_gh cache list -R "$repo" --key '@STATE_PREFIX@' --ref "$ref" --sort created_at --order desc --limit @SELECT_LIMIT@ --json key,createdAt,id,sizeInBytes --jq '.[] | [.createdAt, .key, (.id | tostring), (.sizeInBytes | tostring)] | @tsv' > "$d/list" 2>/dev/null || return 0
  awk -v m=@SELECT_LIMIT@ 'END { exit (NR >= m) }' "$d/list" || return 0
  LC_ALL=C sort -r "$d/list" > "$d/sorted" || return 0
  if command -v timeout >/dev/null 2>&1; then xargs -P @PARALLELISM@ -n 3 timeout "$unit_limit" sh -c "$worker" sh < "$d/units" || :; else xargs -P @PARALLELISM@ -n 3 sh -c "$worker" sh < "$d/units" || :; fi
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
  case "$key" in "$1"*) ;; *) continue ;; esac
  m="${m}x"
  [ "${#m}" -le @LIST_LIMIT@ ] && [ "${#v}" -lt @VERIFY_LIMIT@ ] || break
  case "$key" in *[[:space:]]*) continue ;; esac
  case "$id" in ""|*[!0-9]*) continue ;; esac
  case "$size" in ""|*[!0-9]*) continue ;; esac
  [ "$size" -le "$3" ] || continue
  case "$key" in "$1"?*) ;; *) continue ;; esac
  printf "%s\n" "${key#"$1"}" | grep -Eq "$shape" || continue
  printf "%s %s %s\n" "$key" "$id" "$size" > "$w/subject"
  if run_gh attestation verify "$w/subject" -R "$repo" --signer-workflow "$signer" --source-ref "$ref" --predicate-type "@PREDICATE@" >/dev/null 2>&1; then printf "%s\n" "$key" >> "$w/part"; v="${v}x"; fi
done < "$d/sorted"
mv -f "$w/part" "$w/out"
"#;

const DECIDE: &str = r#"decide() {
  want=false
  if [ -n "$3" ] && [ -n "$4" ]; then
    want=true
    prefix="$2$3-$4-"
    while IFS= read -r cand || [ -n "$cand" ]; do
      case "$cand" in "$prefix"?*) want=false ;; esac
    done < "$d/u-$1/out"
  fi
  printf '%s\n' "$want" > "$d/u-$1/want"
}
wants() {
  w=
  read -r w < "$d/u-$1/want" || :
  [ "$w" = true ]
}
attested() {
  att=
  while IFS= read -r k || [ -n "$k" ]; do att="${att:+$att\\n}$k"; done < "$d/u-$1/out"
}
matrix() {
  if [ -n "$2" ]; then printf '%s\n' "$1={\"include\":[$2]}"; else printf '%s\n' "$1="; fi >> "$GITHUB_OUTPUT"
}
"#;

fn worker() -> Result<String, RenderError> {
    let script = WORKER
        .replace("@RUN_GH@", RUN_GH)
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

fn head() -> Result<String, RenderError> {
    Ok(SELECT_HEAD
        .replace("@RUN_GH@", RUN_GH)
        .replace("@WORKER@", &worker()?)
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

fn outer_bound(unit: &StateUnit) -> Result<u64, RenderError> {
    outer_entry_bound(unit.ceiling().stored()).ok_or_else(|| {
        RenderError::InvalidWorkflow("state_select:outer_bound_unbounded".to_owned())
    })
}

fn json_field(name: &str, value: &str) -> String {
    format!("\\\"{name}\\\":\\\"{value}\\\"")
}

fn matrix_sections(units: &[SealUnit]) -> Result<Vec<String>, RenderError> {
    let mut roots: Vec<&RootSlug> = Vec::new();
    for unit in units {
        if !roots.contains(&&unit.root) {
            roots.push(&unit.root);
        }
    }
    let mut sections = vec!["seal=\npend=0\n".to_owned()];
    sections.extend((0..roots.len()).map(|index| format!("p{index}=\n")));
    for (ordinal, seal) in units.iter().enumerate().rev() {
        let unit = seal.unit();
        let bound = outer_bound(&unit)?;
        let root = roots
            .iter()
            .position(|root| **root == seal.root)
            .ok_or_else(|| RenderError::InvalidWorkflow("state_select:root_missing".to_owned()))?;
        let compat = format!("${{VELNOR_STATE_COMPAT_R{ordinal}-}}");
        let snapshot = format!("${{VELNOR_STATE_SNAPSHOT_R{ordinal}-}}");
        let slice = seal.slice.as_str();
        let sealed = [
            json_field(STATE_MATRIX_SLICE, slice),
            json_field(STATE_MATRIX_COMPAT, &compat),
            json_field(STATE_MATRIX_SNAPSHOT, &snapshot),
            json_field(STATE_MATRIX_PENDING, "$pend"),
        ]
        .join(",");
        let produced = [
            json_field(STATE_MATRIX_SLICE, slice),
            json_field(STATE_MATRIX_SNAPSHOT, &snapshot),
            json_field(STATE_MATRIX_ATTESTED, "$att"),
        ]
        .join(",");
        sections.push(format!(
            "if wants r{ordinal}; then attested r{ordinal}; seal=\"{{{sealed}}}${{seal:+,$seal}}\"; p{root}=\"{{{produced}}}${{p{root}:+,$p{root}}}\"; [ {bound} -le \"$pend\" ] || pend={bound}; fi\n"
        ));
    }
    sections.push(format!("matrix {STATE_SEAL_MATRIX_OUTPUT} \"$seal\"\n"));
    for (index, root) in roots.iter().enumerate() {
        sections.push(format!(
            "matrix {} \"$p{index}\"\n",
            state_producer_matrix_output(root)
        ));
    }
    Ok(sections)
}

pub fn state_select_step(units: &[SealUnit]) -> Result<Step, RenderError> {
    let mut sections = vec![head()?, DECIDE.to_owned()];
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
    let mut decisions = Vec::with_capacity(units.len());
    for (ordinal, seal) in units.iter().enumerate() {
        let unit = seal.unit();
        let compat_env = format!("VELNOR_STATE_COMPAT_R{ordinal}");
        let snapshot_env = format!("VELNOR_STATE_SNAPSHOT_R{ordinal}");
        env.insert(
            compat_env.clone(),
            format!(
                "${{{{ {CACHE_KEYS_OUTPUTS}.{} }}}}",
                state_compat_output(&unit)
            ),
        );
        env.insert(
            snapshot_env.clone(),
            format!(
                "${{{{ {CACHE_KEYS_OUTPUTS}.{} }}}}",
                state_snapshot_output(&unit)
            ),
        );
        sections.push(format!(
            "unit '{}' r{ordinal} {}\n",
            unit_prefix(&unit),
            outer_bound(&unit)?
        ));
        decisions.push(format!(
            "emit {} r{ordinal}\ndecide r{ordinal} '{}' \"${{{compat_env}-}}\" \"${{{snapshot_env}-}}\"\n",
            state_attested_output(&unit),
            unit_prefix(&unit),
        ));
    }
    sections.push("select_all\n".to_owned());
    sections.extend(decisions);
    sections.extend(matrix_sections(units)?);
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
