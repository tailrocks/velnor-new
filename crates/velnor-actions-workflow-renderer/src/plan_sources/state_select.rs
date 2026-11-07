// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, RootSlug, StateScope, StateTrust, StateUnit, outer_entry_bound,
    state_snapshot_output,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_LIST_LIMIT, STATE_SEAL_LIST_TAB, STATE_SEAL_PREDICATE_TYPE,
    STATE_SEAL_SELECT_DEADLINE_DIVISOR, STATE_SEAL_SELECT_DEADLINE_ENV,
    STATE_SEAL_SELECT_LIST_LIMIT, STATE_SEAL_SELECT_MAX_PARALLELISM, STATE_SEAL_UNIT_TIMEOUT_ENV,
    STATE_SEAL_UNIT_TIMEOUT_SECONDS, STATE_SEAL_VERIFY_LIMIT, STATE_SEAL_VERIFY_TIMEOUT_ENV,
    STATE_SEAL_VERIFY_TIMEOUT_SECONDS, STATE_SEAL_WORKFLOW_PATH, state_attested_output,
    state_compat_output, state_produce_output,
};
use velnor_actions_contract::{JobTimeout, Step, StepId, StepKind, StepRole};

use super::cache_keys::DEFAULT_REF_ENV;
use crate::RenderError;
use crate::cache_prune::state_prune_prefix;
use supervisor::SUPERVISOR;

#[path = "state_select_supervisor.rs"]
mod supervisor;

#[path = "state_select_units.rs"]
mod units;

const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const TOKEN_EXPR: &str = "${{ github.token }}";
const STEP_NAME: &str = "Select attested state";
const OUTPUT_DELIMITER: &str = "VELNOR_STATE_ATTESTED_EOF";
const CACHE_KEYS_OUTPUTS: &str = "steps.cache-keys.outputs";
const SUPERVISOR_ENV: &str = "VELNOR_STATE_SELECT_SUPERVISOR_HEX";
const SUPERVISOR_ENV_LIMIT: usize = 49_152;
const SELECT_RUN_LIMIT_UTF16_UNITS: usize = 21_000;

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
  timeout --foreground --kill-after=0.2s "$limit" gh "$@"
}
"#;

const SELECT_HEAD: &str = r#"set -eu
unset i seen line record_name record prefix rest attested produce unit_name root slug remaining character compat_name snapshot_name compat snapshot result extra ordinal state_prefix IFS
command -v printenv >/dev/null 2>&1 || exit 1
case "$RUNNER_TEMP" in /*) ;; *) printf '%s\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac
umask 077
d="$RUNNER_TEMP/velnor/state-select"
rm -rf "$d"
mkdir -p "$d"
chmod 700 "$d"
unset ordinals
ordinals='@ORDINALS@'
IFS=' '
ref="${VELNOR_DEFAULT_REF:-}"
repo="${GITHUB_REPOSITORY:-}"
signer="$repo/@SIGNER_PATH@"
limit="${@TIMEOUT_ENV@:-@TIMEOUT@}"
unit_limit="${@UNIT_TIMEOUT_ENV@:-@UNIT_TIMEOUT@}"
deadline="${@DEADLINE_ENV@:-@DEADLINE@}"
shape='^s256-[0-9a-f]{64}-s256-[0-9a-f]{64}-s256-[0-9a-f]{64}$'
state_prefix='@STATE_PREFIX@'
export d ref repo signer limit shape unit_limit deadline
case "${VELNOR_STATE_SELECT_UNIT_COUNT:-}" in @UNIT_COUNT@) ;; *) exit 1 ;; esac
@RUN_GH@worker='@WORKER@'
export worker
unit() {
  mkdir "$d/u-$2"
  : > "$d/u-$2/out"
  printf '%s %s %s %s %s %s\n' "$1" "$2" "$3" "$4" "$5" "$6" >> "$d/units"
}
select_all() {
  [ -n "$ref" ] && [ -n "$repo" ] && [ -n "$signer" ] || return 0
  command -v python3 >/dev/null 2>&1 || return 0
  command -v bash >/dev/null 2>&1 || return 0
  python3 -I -c 'import math, os, pathlib, signal, subprocess, sys, time' >/dev/null 2>&1 || return 0
@REMOVE_UNIT_ENV@
  export VELNOR_STATE_SELECT_DEADLINE="$deadline" VELNOR_STATE_UNIT_TIMEOUT="$unit_limit" VELNOR_STATE_GH_TIMEOUT="$limit" VELNOR_STATE_PARALLELISM=@PARALLELISM@ VELNOR_STATE_SELECT_DIR="$d" VELNOR_STATE_SELECT_PREFIX="$state_prefix" VELNOR_STATE_SELECT_LIST_LIMIT=@SELECT_LIMIT@
  unset i seen line record_name record prefix rest attested produce unit_name root slug remaining character compat_name snapshot_name compat snapshot result extra ordinal state_prefix IFS
  unset VELNOR_STATE_SELECT_TAG VELNOR_STATE_SELECT_KIND VELNOR_STATE_SELECT_UNIT_PREFIX VELNOR_STATE_SELECT_UNIT
  exec python3 -I -c 'import os; payload = os.environ.pop("VELNOR_STATE_SELECT_SUPERVISOR_HEX"); [os.environ.pop(k) for k in list(os.environ) if k.startswith(("VELNOR_STATE_SELECT_UNIT_R", "VELNOR_STATE_COMPAT_R", "VELNOR_STATE_SNAPSHOT_R"))]; exec(bytes.fromhex(payload))'
}
emit() {
  { printf '%s\n' "$1<<@DELIMITER@"; cat "$d/u-$2/out"; printf '%s\n' @DELIMITER@; } >> "$GITHUB_OUTPUT"
}
"#;

const WORKER: &str = r#"set -eu
unset VELNOR_STATE_SELECT_TAG VELNOR_STATE_SELECT_KIND VELNOR_STATE_SELECT_UNIT_PREFIX VELNOR_STATE_SELECT_UNIT
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
  if run_gh attestation verify "$w/subject" -R "$repo" --signer-workflow "$signer" --source-ref "$ref" --predicate-type "@PREDICATE@" >/dev/null 2>&1; then printf "%s\n" "$key" >> "$w/part"; v="${v}x"; else status=$?; case "$status" in 124|125|126|127|137) exit "$status" ;; esac; fi
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

struct SelectBounds {
    units: usize,
    parallelism: usize,
    unit_seconds: u64,
    deadline_seconds: u64,
}

impl SelectBounds {
    fn of(units: usize, plan_timeout: JobTimeout) -> Self {
        let parallelism = units.clamp(1, STATE_SEAL_SELECT_MAX_PARALLELISM);
        let batches = units.div_ceil(parallelism).max(1) as u64;
        let deadline_seconds =
            (u64::from(plan_timeout.minutes()) * 60 / STATE_SEAL_SELECT_DEADLINE_DIVISOR).max(1);
        let unit_seconds = STATE_SEAL_UNIT_TIMEOUT_SECONDS
            .min(deadline_seconds / batches)
            .max(1);
        Self {
            units,
            parallelism,
            unit_seconds,
            deadline_seconds,
        }
    }
}

fn head(outer_bound: u64, bounds: &SelectBounds) -> Result<String, RenderError> {
    Ok(SELECT_HEAD
        .replace("@RUN_GH@", RUN_GH)
        .replace("@WORKER@", &worker(outer_bound)?)
        .replace("@TIMEOUT_ENV@", STATE_SEAL_VERIFY_TIMEOUT_ENV)
        .replace("@TIMEOUT@", &STATE_SEAL_VERIFY_TIMEOUT_SECONDS.to_string())
        .replace("@UNIT_TIMEOUT_ENV@", STATE_SEAL_UNIT_TIMEOUT_ENV)
        .replace("@UNIT_TIMEOUT@", &bounds.unit_seconds.to_string())
        .replace("@DEADLINE_ENV@", STATE_SEAL_SELECT_DEADLINE_ENV)
        .replace("@DEADLINE@", &bounds.deadline_seconds.to_string())
        .replace("@STATE_PREFIX@", &state_prune_prefix())
        .replace(
            "@ORDINALS@",
            &(0..bounds.units)
                .map(|ordinal| ordinal.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        )
        .replace("@SELECT_LIMIT@", &STATE_SEAL_SELECT_LIST_LIMIT.to_string())
        .replace("@PARALLELISM@", &bounds.parallelism.to_string())
        .replace("@UNIT_COUNT@", &bounds.units.to_string())
        .replace("@REMOVE_UNIT_ENV@", units::REMOVE_UNIT_ENV)
        .replace("@DELIMITER@", OUTPUT_DELIMITER)
        .replace("@SIGNER_PATH@", STATE_SEAL_WORKFLOW_PATH))
}

fn supervisor_hex(bounds: &SelectBounds) -> Result<String, RenderError> {
    let trusted_deadline_ceiling = 3600_u64.max(bounds.deadline_seconds);
    let source = SUPERVISOR.replace("@MAX_DEADLINE@", &trusted_deadline_ceiling.to_string());
    let mut encoded = String::with_capacity(source.len() * 2);
    for byte in source.bytes() {
        encoded.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or_default());
        encoded.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or_default());
    }
    if encoded.len() > SUPERVISOR_ENV_LIMIT {
        return Err(RenderError::InvalidWorkflow(
            "state_select:supervisor_env_too_large".to_owned(),
        ));
    }
    Ok(encoded)
}

fn validate_selector_run(step: &Step) -> Result<(), RenderError> {
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "state_select:shell_step_required".to_owned(),
        ));
    };
    let joined_run = crate::commands::join_validated_argv_for_run(run)?;
    if joined_run.encode_utf16().count() > SELECT_RUN_LIMIT_UTF16_UNITS {
        return Err(RenderError::InvalidWorkflow(
            "state_select:run_too_large:21000_utf16_code_units".to_owned(),
        ));
    }
    Ok(())
}

pub fn state_select_step(
    roots: &[SealRoot],
    plan_timeout: JobTimeout,
) -> Result<Step, RenderError> {
    let outer_bound = outer_entry_bound(CacheFamily::MbxBundle.spec().ceiling.stored())
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("state_select:outer_bound_unbounded".to_owned())
        })?;
    let bounds = SelectBounds::of(roots.len(), plan_timeout);
    let mut sections = vec![head(outer_bound, &bounds)?, DECIDE.to_owned()];
    let mut env = BTreeMap::from([
        (DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned()),
        ("GH_TOKEN".to_owned(), TOKEN_EXPR.to_owned()),
        (
            STATE_SEAL_VERIFY_TIMEOUT_ENV.to_owned(),
            STATE_SEAL_VERIFY_TIMEOUT_SECONDS.to_string(),
        ),
        (
            STATE_SEAL_UNIT_TIMEOUT_ENV.to_owned(),
            bounds.unit_seconds.to_string(),
        ),
        (
            STATE_SEAL_SELECT_DEADLINE_ENV.to_owned(),
            bounds.deadline_seconds.to_string(),
        ),
    ]);
    env.insert(SUPERVISOR_ENV.to_owned(), supervisor_hex(&bounds)?);
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
        let record = format!(
            "{} {} {}",
            root.prefix(),
            state_attested_output(root.unit.as_str(), root.slug.as_str()),
            state_produce_output(root.unit.as_str(), root.slug.as_str()),
        );
        if record.len() > 512 {
            return Err(RenderError::InvalidWorkflow(
                "state_select:unit_record_too_large".to_owned(),
            ));
        }
        env.insert(format!("VELNOR_STATE_SELECT_UNIT_R{ordinal}"), record);
    }
    env.insert(
        "VELNOR_STATE_SELECT_UNIT_COUNT".to_owned(),
        roots.len().to_string(),
    );
    sections.push(units::PREPARE_UNITS.to_owned());
    sections.push("select_all\n".to_owned());
    sections.push(units::COLD_UNITS.to_owned());
    let script = sections.concat();
    let mut step = crate::steps::ambient_shell_step(
        STEP_NAME,
        vec![
            "exec".to_owned(),
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script),
        ],
        env,
    )?;
    validate_selector_run(&step)?;
    step.id = Some(StepId::StateSelect);
    step.role = Some(StepRole::StateSelect);
    Ok(step)
}
