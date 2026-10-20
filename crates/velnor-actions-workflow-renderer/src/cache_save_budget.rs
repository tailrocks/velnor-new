//! Best-effort native cache writes share the first user-step clock.
//!
//! This is not the runner's job-start clock. The fixed reserve accounts for
//! startup and cleanup conservatively; it does not guarantee whole-job fit.

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, yaml::Yaml};

const CLOCK_ID: &str = "velnor-cache-clock";
const SOURCES_ID: &str = "velnor-optional-sources-save";
const TOOLS_ID: &str = "velnor-optional-tools-save";
const PROVIDERS_ID: &str = "velnor-optional-providers-save";
const RESERVE_SECONDS: u32 = 30;
const WRITE_SECONDS: u32 = 60;

const CLOCK_SCRIPT: &str = r#"set -eu
unavailable() { printf '%s\n' '::warning::cache_unavailable: invalid first-step clock or output'; }
output=${GITHUB_OUTPUT-}
seconds=${VELNOR_CACHE_JOB_SECONDS-}
now=$(date +%s) || now=
for value in "$now" "$seconds"; do
  case "$value" in ''|*[!0-9]*) unavailable; exit 0 ;; esac
  case "$value" in 0|[1-9]*) ;; *) unavailable; exit 0 ;; esac
  [ "${#value}" -le 10 ] || { unavailable; exit 0; }
done
if [ -z "$output" ] || ! printf 'started=%s\ndeadline=%s\n' "$now" "$((now + seconds))" 2>/dev/null >> "$output"; then
  unavailable
fi
"#;

const ADMISSION_SCRIPT: &str = r#"set -eu
output=${GITHUB_OUTPUT-}
emit() { [ -n "$output" ] && printf '%s\n' "$1" 2>/dev/null >> "$output"; }
refuse() {
  emit 'admitted=false' || :
  printf '%s\n' '::warning::cache_unavailable: optional save has no safe remaining job budget or output'
}
started=${VELNOR_CACHE_STARTED-}
deadline=${VELNOR_CACHE_DEADLINE-}
seconds=${VELNOR_CACHE_JOB_SECONDS-}
required=${VELNOR_CACHE_REQUIRED_SECONDS-}
now=$(date +%s) || now=
for value in "$started" "$deadline" "$now" "$seconds" "$required"; do
  case "$value" in ''|*[!0-9]*) refuse; exit 0 ;; esac
  case "$value" in 0|[1-9]*) ;; *) refuse; exit 0 ;; esac
  [ "${#value}" -le 10 ] || { refuse; exit 0; }
done
if [ "$now" -lt "$started" ] || [ "$deadline" -lt "$started" ] ||
   [ "$((deadline - started))" -ne "$seconds" ] ||
   [ "$((deadline - now))" -lt "$required" ]; then
  refuse
elif ! emit 'admitted=true'; then
  refuse
fi
"#;

/// Only exact native job save actions may receive the optional-write exception.
pub(crate) fn decorate(
    step: &Step,
    entries: &mut Vec<(String, Yaml)>,
    job_context: bool,
) -> Result<(), RenderError> {
    let id = match step.role {
        Some(StepRole::CargoSourcesSave) => SOURCES_ID,
        Some(StepRole::ToolsCacheSave) => TOOLS_ID,
        Some(StepRole::TofuProvidersSave) => PROVIDERS_ID,
        _ => return Ok(()),
    };
    let StepKind::Action { uses, .. } = &step.kind else {
        return Err(invalid());
    };
    if !job_context || uses != crate::steps::TOOLS_SAVE_USES || step.id.is_some() || step.parallel {
        return Err(invalid());
    }
    if step.condition.is_none() || field(entries, "if").is_none() {
        return Err(invalid());
    }
    entries.extend([
        ("id".to_owned(), Yaml::str(id)),
        ("timeout-minutes".to_owned(), Yaml::Int(1)),
        ("continue-on-error".to_owned(), Yaml::Bool(true)),
    ]);
    Ok(())
}

/// Add one clock before the original job steps and admission before each save.
pub(crate) fn apply(steps: &mut Vec<Yaml>, minutes: u16) -> Result<(), RenderError> {
    validate_ids(steps)?;
    let mut output = Vec::with_capacity(steps.len() + 5);
    let mut saves = false;
    for mut step in std::mem::take(steps) {
        if let Some(id) = save_id(&step).map(str::to_owned) {
            let Yaml::Map(entries) = &mut step else {
                return Err(invalid());
            };
            let Some((_, Yaml::Str(condition))) = entries.iter_mut().find(|(key, _)| key == "if")
            else {
                return Err(invalid());
            };
            output.push(admission(&id, minutes, condition));
            *condition = format!("({condition}) && steps.{id}-budget.outputs.admitted == 'true'");
            output.push(step);
            output.push(outcome(&id));
            saves = true;
        } else {
            output.push(step);
        }
    }
    if saves {
        output.insert(0, clock(minutes));
    }
    *steps = output;
    Ok(())
}

fn validate_ids(steps: &[Yaml]) -> Result<(), RenderError> {
    let mut seen = std::collections::BTreeSet::new();
    for step in steps {
        let Some(id) = step_id(step) else { continue };
        if id == CLOCK_ID
            || id == format!("{SOURCES_ID}-budget")
            || id == format!("{TOOLS_ID}-budget")
            || id == format!("{PROVIDERS_ID}-budget")
        {
            return Err(invalid());
        }
        if save_id(step).is_some() && !seen.insert(id) {
            return Err(invalid());
        }
    }
    Ok(())
}

fn field<'a>(entries: &'a [(String, Yaml)], key: &str) -> Option<&'a Yaml> {
    entries
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn step_id(step: &Yaml) -> Option<&str> {
    let Yaml::Map(entries) = step else {
        return None;
    };
    match field(entries, "id")? {
        Yaml::Str(id) | Yaml::Quoted(id) | Yaml::Annotated { value: id, .. } => Some(id),
        _ => None,
    }
}

fn save_id(step: &Yaml) -> Option<&str> {
    step_id(step).filter(|id| matches!(*id, SOURCES_ID | TOOLS_ID | PROVIDERS_ID))
}

fn clock(minutes: u16) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Record optional cache budget")),
        ("id".to_owned(), Yaml::str(CLOCK_ID)),
        ("run".to_owned(), Yaml::str(CLOCK_SCRIPT)),
        ("env".to_owned(), Yaml::Map(job_seconds(minutes))),
    ])
}

fn job_seconds(minutes: u16) -> Vec<(String, Yaml)> {
    vec![(
        "VELNOR_CACHE_JOB_SECONDS".to_owned(),
        Yaml::str((u32::from(minutes) * 60).to_string()),
    )]
}

fn admission(id: &str, minutes: u16, condition: &str) -> Yaml {
    let mut env = job_seconds(minutes);
    env.extend([
        (
            "VELNOR_CACHE_STARTED".to_owned(),
            Yaml::str(format!("${{{{ steps.{CLOCK_ID}.outputs.started }}}}")),
        ),
        (
            "VELNOR_CACHE_DEADLINE".to_owned(),
            Yaml::str(format!("${{{{ steps.{CLOCK_ID}.outputs.deadline }}}}")),
        ),
        (
            "VELNOR_CACHE_REQUIRED_SECONDS".to_owned(),
            Yaml::str((WRITE_SECONDS + RESERVE_SECONDS).to_string()),
        ),
    ]);
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Admit optional cache save")),
        ("id".to_owned(), Yaml::str(format!("{id}-budget"))),
        ("if".to_owned(), Yaml::str(condition)),
        ("run".to_owned(), Yaml::str(ADMISSION_SCRIPT)),
        ("env".to_owned(), Yaml::Map(env)),
    ])
}

fn outcome(id: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Report optional cache save failure"),
        ),
        (
            "if".to_owned(),
            Yaml::str(format!(
                "always() && !cancelled() && steps.{id}.outcome == 'failure'"
            )),
        ),
        (
            "run".to_owned(),
            Yaml::str(
                "printf '%s\\n' '::warning::cache_unavailable: optional save failed or timed out'",
            ),
        ),
    ])
}

fn invalid() -> RenderError {
    RenderError::InvalidWorkflow("invalid_optional_cache_save_budget".to_owned())
}

#[cfg(test)]
#[path = "cache_save_budget_tests.rs"]
mod tests;
