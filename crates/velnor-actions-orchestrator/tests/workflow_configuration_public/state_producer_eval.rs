// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::{Job, StepKind, StepRole};

type Needs = BTreeMap<(String, String), String>;

struct Run<'a> {
    failed: bool,
    outputs: BTreeMap<(&'a str, &'a str), &'static str>,
    outcomes: BTreeMap<&'a str, &'static str>,
}

fn unmodelled(what: &str) -> Box<dyn Error> {
    format!("the condition evaluator does not model {what}").into()
}

fn holds(atom: &str, run: &Run<'_>, needs: &Needs) -> Result<bool, Box<dyn Error>> {
    let atom = if atom.ends_with("()") {
        atom
    } else {
        atom.trim_start_matches('(').trim_end_matches(')')
    };
    match atom {
        "always()" | "!cancelled()" => return Ok(true),
        "success()" => return Ok(!run.failed),
        "failure()" => return Ok(run.failed),
        _ => {}
    }
    if atom.starts_with("github.") {
        return Ok(true);
    }
    let (left, equals, right) = if let Some((left, right)) = atom.split_once(" == ") {
        (left, true, right)
    } else if let Some((left, right)) = atom.split_once(" != ") {
        (left, false, right)
    } else {
        return Err(unmodelled(atom));
    };
    let value = right.trim_matches('\'');
    let actual = if let Some(rest) = left.strip_prefix("steps.") {
        if let Some((id, name)) = rest.split_once(".outputs.") {
            run.outputs
                .get(&(id, name))
                .copied()
                .unwrap_or("")
                .to_owned()
        } else if let Some(id) = rest.strip_suffix(".outcome") {
            run.outcomes
                .get(id)
                .copied()
                .unwrap_or("skipped")
                .to_owned()
        } else {
            return Err(unmodelled(left));
        }
    } else if let Some((job, name)) = left
        .strip_prefix("needs.")
        .and_then(|rest| rest.split_once(".outputs."))
    {
        needs
            .get(&(job.to_owned(), name.to_owned()))
            .cloned()
            .unwrap_or_default()
    } else {
        return Err(unmodelled(left));
    };
    Ok((actual == value) == equals)
}

fn evaluates(
    condition: Option<&str>,
    run: &Run<'_>,
    needs: &Needs,
) -> Result<bool, Box<dyn Error>> {
    let Some(condition) = condition else {
        return Ok(!run.failed);
    };
    let atoms: Vec<&str> = condition.split(" && ").collect();
    let status = atoms
        .iter()
        .any(|atom| ["always()", "!cancelled()", "success()", "failure()"].contains(atom));
    for atom in &atoms {
        if !holds(atom, run, needs)? {
            return Ok(false);
        }
    }
    Ok(status || !run.failed)
}

pub(super) fn simulate(
    producer: &Job,
    failing: Option<usize>,
    refuse_export: bool,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    Ok(simulate_run(producer, failing, refuse_export, false)?.0)
}

pub(super) fn simulate_run(
    producer: &Job,
    failing: Option<usize>,
    refuse_export: bool,
    stale_tip: bool,
) -> Result<(BTreeMap<String, String>, usize), Box<dyn Error>> {
    let mut run = Run {
        failed: false,
        outputs: BTreeMap::from([(("mbx-bundle-key", "save"), "true")]),
        outcomes: BTreeMap::new(),
    };
    let mut lane_index = 0;
    let mut lanes_run = 0;
    let none = Needs::new();
    for step in &producer.steps {
        if !evaluates(step.condition.as_deref(), &run, &none)? {
            if matches!(step.kind, StepKind::TaskExecution { .. }) {
                lane_index += 1;
            }
            continue;
        }
        if matches!(step.kind, StepKind::TaskExecution { .. }) {
            lanes_run += 1;
            if failing == Some(lane_index) {
                run.failed = true;
            }
            lane_index += 1;
            continue;
        }
        if step.role == Some(StepRole::StateSealTip) && stale_tip {
            run.failed = true;
            continue;
        }
        match step.id.map(velnor_actions_contract::StepId::as_str) {
            Some("mbx-bundle-export") if !refuse_export => {
                run.outputs
                    .insert(("mbx-bundle-export", "exported"), "true");
            }
            Some("state-seal-upload") => {
                run.outcomes.insert("state-seal-upload", "success");
                run.outputs
                    .insert(("state-seal-upload", "artifact-id"), "7001");
            }
            Some("state-produced") => {
                run.outputs.insert(("state-produced", "produced"), "true");
            }
            _ => {}
        }
    }
    let mut outputs = BTreeMap::new();
    for (name, expression) in &producer.outputs {
        let inner = expression
            .trim_start_matches("${{ ")
            .trim_end_matches(" }}");
        let (step, output) = inner
            .strip_prefix("steps.")
            .and_then(|rest| rest.split_once(".outputs."))
            .ok_or_else(|| unmodelled(expression))?;
        let value = run.outputs.get(&(step, output)).copied().unwrap_or("");
        outputs.insert(name.clone(), value.to_owned());
    }
    Ok((outputs, lanes_run))
}

pub(super) fn seal_runs(
    seal: &Job,
    producer_id: &str,
    outputs: &BTreeMap<String, String>,
) -> Result<bool, Box<dyn Error>> {
    let needs: Needs = outputs
        .iter()
        .map(|(name, value)| ((producer_id.to_owned(), name.clone()), value.clone()))
        .collect();
    let run = Run {
        failed: false,
        outputs: BTreeMap::new(),
        outcomes: BTreeMap::new(),
    };
    evaluates(seal.condition.as_deref(), &run, &needs)
}

pub(super) fn seal_steps_run(
    seal: &Job,
    admitted: bool,
    tip_current: bool,
) -> Result<Vec<StepRole>, Box<dyn Error>> {
    let mut run = Run {
        failed: false,
        outputs: BTreeMap::from([(("state-seal-subject", "sealed"), "true")]),
        outcomes: BTreeMap::new(),
    };
    let none = Needs::new();
    let mut ran = Vec::new();
    for step in &seal.steps {
        if !evaluates(step.condition.as_deref(), &run, &none)? {
            continue;
        }
        let Some(role) = step.role else {
            continue;
        };
        ran.push(role);
        match role {
            StepRole::StateSealTip => {
                run.outputs.insert(
                    ("state-seal-tip", "current"),
                    if tip_current { "true" } else { "false" },
                );
            }
            StepRole::StateSealAdmit => {
                run.outputs.insert(
                    ("state-seal-admit", "admitted"),
                    if admitted { "true" } else { "false" },
                );
            }
            StepRole::StateSealSave => {
                run.outcomes.insert("state-seal-save", "success");
            }
            StepRole::StateSealBind => {
                run.outputs.insert(("state-seal-bind", "bound"), "true");
            }
            _ => {}
        }
    }
    Ok(ran)
}

pub(super) fn writer_roles_run(
    writer: &Job,
    stale_tip: bool,
) -> Result<Vec<StepRole>, Box<dyn Error>> {
    let mut run = Run {
        failed: false,
        outputs: BTreeMap::from([(("v2", "enabled"), "true")]),
        outcomes: BTreeMap::new(),
    };
    let none = Needs::new();
    let mut ran = Vec::new();
    for step in &writer.steps {
        if !evaluates(step.condition.as_deref(), &run, &none)? {
            continue;
        }
        if let Some(role) = step.role {
            ran.push(role);
        }
        if step.role == Some(StepRole::StateSealTip) && stale_tip {
            run.failed = true;
        }
    }
    Ok(ran)
}
