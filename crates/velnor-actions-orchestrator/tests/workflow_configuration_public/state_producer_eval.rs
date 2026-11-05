// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::workflow::state_seal::state_producer_lane_slice;
use velnor_actions_contract::{Job, StepKind, StepRole};

type Needs = BTreeMap<(String, String), String>;

struct Run<'a> {
    leg: Option<String>,
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
    if let Some(slice) = atom
        .strip_prefix("matrix.slice == '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return Ok(run.leg.as_deref() == Some(slice));
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

pub(super) struct LegRun {
    pub(super) lanes: usize,
    pub(super) uploaded: bool,
}

pub(super) struct LegFaults {
    pub(super) failing: Option<usize>,
    pub(super) refuse_export: bool,
    pub(super) stale_tip: bool,
}

pub(super) const HEALTHY: LegFaults = LegFaults {
    failing: None,
    refuse_export: false,
    stale_tip: false,
};

pub(super) fn legs_of(producer: &Job) -> BTreeSet<String> {
    producer
        .steps
        .iter()
        .filter_map(|step| {
            step.condition
                .as_deref()
                .and_then(state_producer_lane_slice)
        })
        .map(|slice| slice.to_string())
        .collect()
}

pub(super) fn simulate_leg(
    producer: &Job,
    leg: &str,
    faults: &LegFaults,
) -> Result<LegRun, Box<dyn Error>> {
    let mut run = Run {
        leg: Some(leg.to_owned()),
        failed: false,
        outputs: BTreeMap::from([(("mbx-bundle-key", "save"), "true")]),
        outcomes: BTreeMap::new(),
    };
    let mut lanes = 0;
    let none = Needs::new();
    for step in &producer.steps {
        if !evaluates(step.condition.as_deref(), &run, &none)? {
            continue;
        }
        if matches!(step.kind, StepKind::TaskExecution { .. }) {
            if faults.failing == Some(lanes) {
                run.failed = true;
            }
            lanes += 1;
            continue;
        }
        if step.role == Some(StepRole::StateSealTip) && faults.stale_tip {
            run.failed = true;
            continue;
        }
        match step.id.map(velnor_actions_contract::StepId::as_str) {
            Some("mbx-bundle-export") if !faults.refuse_export => {
                run.outputs
                    .insert(("mbx-bundle-export", "exported"), "true");
            }
            Some("state-seal-upload") => {
                run.outcomes.insert("state-seal-upload", "success");
            }
            _ => {}
        }
    }
    Ok(LegRun {
        lanes,
        uploaded: run.outcomes.get("state-seal-upload") == Some(&"success"),
    })
}

pub(super) fn seal_steps_run(
    seal: &Job,
    admitted: bool,
    tip_current: bool,
) -> Result<Vec<StepRole>, Box<dyn Error>> {
    let mut run = Run {
        leg: None,
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
        leg: None,
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
