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
use velnor_actions_contract::{JobTimeout, Step, StepId, StepRole};

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
  if command -v timeout >/dev/null 2>&1; then timeout --foreground "$limit" gh "$@"; else gh "$@"; fi
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
deadline="${@DEADLINE_ENV@:-@DEADLINE@}"
shape='^s256-[0-9a-f]{64}-s256-[0-9a-f]{64}-s256-[0-9a-f]{64}$'
export d ref repo signer limit shape unit_limit deadline
@RUN_GH@worker='@WORKER@'
export worker
unit() {
  mkdir "$d/u-$2"
  : > "$d/u-$2/out"
  printf '%s %s\n' "$1" "$2" >> "$d/units"
}
select_all() {
  [ -n "$ref" ] && [ -n "$repo" ] && [ -n "$signer" ] || return 0
  command -v python3 >/dev/null 2>&1 || return 0
  command -v bash >/dev/null 2>&1 || return 0
  export VELNOR_STATE_SELECT_DEADLINE="$deadline" VELNOR_STATE_UNIT_TIMEOUT="$unit_limit" VELNOR_STATE_PARALLELISM=@PARALLELISM@ VELNOR_STATE_SELECT_DIR="$d" VELNOR_STATE_SELECT_PREFIX='@STATE_PREFIX@' VELNOR_STATE_SELECT_LIST_LIMIT=@SELECT_LIMIT@
  if @SUPERVISOR_COMMAND@; then :; else
    for unit_dir in "$d"/u-*; do [ -d "$unit_dir" ] && : > "$unit_dir/out"; done
  fi
}
emit() {
  { printf '%s\n' "$1<<@DELIMITER@"; cat "$d/u-$2/out"; printf '%s\n' @DELIMITER@; } >> "$GITHUB_OUTPUT"
}
"#;

const SUPERVISOR: &str = r#"import os
import math
import pathlib
import signal
import subprocess
import sys
import time

d = pathlib.Path(os.environ["VELNOR_STATE_SELECT_DIR"])
deadline = float(os.environ["VELNOR_STATE_SELECT_DEADLINE"])
unit_timeout = float(os.environ["VELNOR_STATE_UNIT_TIMEOUT"])
parallelism = int(os.environ["VELNOR_STATE_PARALLELISM"])
list_limit = int(os.environ["VELNOR_STATE_SELECT_LIST_LIMIT"])
if not math.isfinite(deadline) or not math.isfinite(unit_timeout) or not 0 < deadline <= 3600 or not 0 < unit_timeout <= 3600 or not 1 <= parallelism <= 32 or not 0 < list_limit <= 1000:
    sys.exit(1)
started = time.monotonic()
active = {}
guardian = """d=$VELNOR_STATE_SELECT_DIR
tag=$VELNOR_STATE_SELECT_TAG
kind=$VELNOR_STATE_SELECT_KIND
prefix=$VELNOR_STATE_SELECT_UNIT_PREFIX
unit=$VELNOR_STATE_SELECT_UNIT
pgid=$$
trap ':' TERM
while [ ! -e "$d/ready-$pgid" ]; do sleep 0.01; done
if [ "$kind" = list ]; then
  gh cache list -R "$GITHUB_REPOSITORY" --key "$VELNOR_STATE_SELECT_PREFIX" --ref "$VELNOR_DEFAULT_REF" --sort created_at --order desc --limit "$VELNOR_STATE_SELECT_LIST_LIMIT" --json key,createdAt,id,sizeInBytes --jq '.[] | [.createdAt, .key, (.id | tostring), (.sizeInBytes | tostring)] | @tsv' > "$d/list" 2>/dev/null
  result=$?
else
  bash -c "$worker" bash "$prefix" "$unit" "$pgid"
  result=$?
fi
printf '%s\\n' "$result" > "$d/status-$tag"
while [ ! -e "$d/release-$pgid" ]; do sleep 0.01; done
exit 0"""

class AggregateExpired(Exception):
    pass

def clear_outputs():
    for output in d.glob("u-*/out"):
        output.write_text("")

def stop_groups(tags):
    procs = [active[tag][0] for tag in tags if tag in active]
    for proc in procs:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    if procs:
        time.sleep(0.2)
    for proc in procs:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    for tag in tags:
        proc = active.pop(tag, None)
        if proc is not None:
            proc[0].wait()
            (d / f"ready-{proc[0].pid}").unlink(missing_ok=True)
            (d / f"release-{proc[0].pid}").unlink(missing_ok=True)
            (d / f"status-{tag}").unlink(missing_ok=True)

def on_signal(signum, frame):
    raise AggregateExpired()

for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
    signal.signal(sig, on_signal)

def reset_child_signals():
    signal.pthread_sigmask(signal.SIG_SETMASK, set())

def launch(tag, kind, prefix="", unit=""):
    old_mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT, signal.SIGHUP})
    env = os.environ.copy()
    env.update({
        "VELNOR_STATE_SELECT_TAG": tag,
        "VELNOR_STATE_SELECT_KIND": kind,
        "VELNOR_STATE_SELECT_UNIT_PREFIX": prefix,
        "VELNOR_STATE_SELECT_UNIT": unit,
    })
    proc = subprocess.Popen(
        ["bash", "-c", guardian],
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
        preexec_fn=reset_child_signals,
    )
    active[tag] = (proc, time.monotonic())
    (d / f"ready-{proc.pid}").touch()
    signal.pthread_sigmask(signal.SIG_SETMASK, old_mask)
    return proc

def check_deadline():
    if time.monotonic() - started >= deadline:
        raise AggregateExpired()

def await_group(tag):
    while True:
        check_deadline()
        status_path = d / f"status-{tag}"
        if status_path.exists():
            try:
                return int(status_path.read_text().strip())
            except ValueError:
                return 1
        time.sleep(0.01)

def finish(tags):
    stop_groups(tags)

def main():
    launch("list", "list")
    list_status = await_group("list")
    finish(["list"])
    if list_status != 0:
        return
    rows = (d / "list").read_text().splitlines()
    if len(rows) >= list_limit:
        return
    (d / "sorted").write_text("\n".join(sorted(rows, reverse=True)) + ("\n" if rows else ""))
    units = [line.split(" ", 1) for line in (d / "units").read_text().splitlines()]
    next_unit = 0
    while next_unit < len(units) or active:
        check_deadline()
        while next_unit < len(units) and len(active) < parallelism:
            prefix, unit = units[next_unit]
            tag = f"u{next_unit}"
            (d / f"u-{unit}" / "out").write_text("")
            launch(tag, "unit", prefix, unit)
            next_unit += 1
        finished = []
        expired_units = []
        now = time.monotonic()
        for tag, (_, spawned) in list(active.items()):
            status = d / f"status-{tag}"
            if status.exists():
                finished.append(tag)
            elif tag != "list" and now - spawned >= unit_timeout:
                unit = units[int(tag[1:])][1]
                finished.append(tag)
                expired_units.append(unit)
        if finished:
            finish(finished)
            for unit in expired_units:
                (d / f"u-{unit}" / "out").write_text("")
        elif active:
            time.sleep(0.01)

try:
    main()
except AggregateExpired:
    stop_groups(list(active))
    clear_outputs()
    sys.exit(124)
except BaseException:
    stop_groups(list(active))
    clear_outputs()
    raise
finally:
    if active:
        stop_groups(list(active))
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
  if run_gh attestation verify "$w/subject" -R "$repo" --signer-workflow "$signer" --source-ref "$ref" --predicate-type "@PREDICATE@" >/dev/null 2>&1; then printf "%s\n" "$key" >> "$w/part"; v="${v}x"; else status=$?; [ "$status" -ne 124 ] || exit "$status"; fi
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
            .min(STATE_SEAL_VERIFY_TIMEOUT_SECONDS.saturating_sub(1))
            .max(1);
        Self {
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
        .replace("@SUPERVISOR_COMMAND@", &supervisor_command())
        .replace("@TIMEOUT_ENV@", STATE_SEAL_VERIFY_TIMEOUT_ENV)
        .replace("@TIMEOUT@", &STATE_SEAL_VERIFY_TIMEOUT_SECONDS.to_string())
        .replace("@UNIT_TIMEOUT_ENV@", STATE_SEAL_UNIT_TIMEOUT_ENV)
        .replace("@UNIT_TIMEOUT@", &bounds.unit_seconds.to_string())
        .replace("@DEADLINE_ENV@", STATE_SEAL_SELECT_DEADLINE_ENV)
        .replace("@DEADLINE@", &bounds.deadline_seconds.to_string())
        .replace("@STATE_PREFIX@", &state_prune_prefix())
        .replace("@SELECT_LIMIT@", &STATE_SEAL_SELECT_LIST_LIMIT.to_string())
        .replace("@PARALLELISM@", &bounds.parallelism.to_string())
        .replace("@DELIMITER@", OUTPUT_DELIMITER)
        .replace("@SIGNER_PATH@", STATE_SEAL_WORKFLOW_PATH))
}

fn supervisor_command() -> String {
    let source = SUPERVISOR
        .bytes()
        .map(|byte| format!("\\0{byte:03o}"))
        .collect::<String>();
    format!("printf '%b' \"{source}\" | python3 -I -")
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
