// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use serde_json::Value;

use crate::pairs::{Decision, slots};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub name: String,
    pub up_seconds: u64,
    pub mode: &'static str,
    pub authority: String,
    pub ledger: usize,
    pub pairs: Vec<String>,
    pub refusal: Option<String>,
}

fn plan_slots(plan: &str) -> Result<Vec<String>, String> {
    let bad = |what: &str| format!("runner/plan.json: {what}");
    let plan: Value = serde_json::from_str(plan).map_err(|error| bad(&error.to_string()))?;
    let sides = plan["sides"]
        .as_array()
        .ok_or_else(|| bad("`sides` is missing or not an array"))?;
    let mut counts = [0_u16; 2];
    for entry in sides {
        let side = entry["side"]
            .as_str()
            .ok_or_else(|| bad("a `sides` entry has no `side` name"))?;
        let pairs = entry["pairs"]
            .as_u64()
            .and_then(|pairs| u16::try_from(pairs).ok())
            .ok_or_else(|| bad(&format!("`pairs` of `{side}` is not a count")))?;
        match side {
            "trusted" => counts[0] = pairs,
            "untrusted" => counts[1] = pairs,
            other => return Err(bad(&format!("unknown side `{other}`"))),
        }
    }
    Ok(slots(counts[0], counts[1]))
}

fn named(value: Option<impl ToString>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

fn current(log: &str, slot: &str) -> Option<Decision> {
    let (listening, removed) = (
        format!("pair {slot} listening"),
        format!("pair {slot} removed"),
    );
    let mut busy = None;
    for line in log.lines() {
        if line == listening || line == removed {
            busy = None;
        } else if let Some(decision) = line
            .strip_prefix("decision ")
            .and_then(|json| serde_json::from_str::<Decision>(json).ok())
            .filter(|decision| decision.pair == slot)
        {
            busy = Some(decision).filter(|decision| decision.allow);
        }
    }
    busy
}

pub fn pair_states(plan: &str, log: &str) -> Result<Vec<String>, String> {
    Ok(plan_slots(plan)?
        .into_iter()
        .map(|slot| match current(log, &slot) {
            Some(last) => {
                let (run, job) = (named(last.run_id), named(last.job.as_deref()));
                format!("{slot} busy (run {run}, job {job})")
            }
            None => format!("{slot} idle"),
        })
        .collect())
}

#[must_use]
pub fn last_refusal(decisions: &[Decision]) -> Option<String> {
    decisions
        .iter()
        .rfind(|decision| !decision.allow)
        .map(|decision| {
            let (run, job) = (named(decision.run_id), named(decision.job.as_deref()));
            let reason = decision.reason.as_deref().unwrap_or("no reason given");
            format!("run {run} job {job}: {reason}")
        })
}

fn age(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => format!("{} h {} min", seconds / 3600, seconds % 3600 / 60),
    }
}

#[must_use]
pub fn render(snapshot: &Snapshot) -> String {
    let events = if snapshot.ledger == 1 {
        "event"
    } else {
        "events"
    };
    let mut text = format!(
        "{}: up {}, runner mode {}, authority {}, ledger {} {events}\n",
        snapshot.name,
        age(snapshot.up_seconds),
        snapshot.mode,
        snapshot.authority,
        snapshot.ledger
    );
    let pairs = if snapshot.pairs.is_empty() {
        "none (plain containers)".to_owned()
    } else {
        snapshot.pairs.join("  ")
    };
    writeln!(text, "  pairs: {pairs}").ok();
    writeln!(
        text,
        "  last refusal: {}",
        snapshot.refusal.as_deref().unwrap_or("none")
    )
    .ok();
    text
}
