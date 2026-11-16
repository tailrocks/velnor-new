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
    let plan: Value =
        serde_json::from_str(plan).map_err(|error| format!("runner/plan.json: {error}"))?;
    let count = |side: &str| {
        plan["sides"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["side"] == side)
            .and_then(|entry| entry["pairs"].as_u64())
            .and_then(|pairs| u16::try_from(pairs).ok())
            .unwrap_or(0)
    };
    Ok(slots(count("trusted"), count("untrusted")))
}

fn named(value: Option<impl ToString>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

pub fn pair_states(plan: &str, log: &str, decisions: &[Decision]) -> Result<Vec<String>, String> {
    Ok(plan_slots(plan)?
        .into_iter()
        .map(|slot| {
            let prefix = format!("pair {slot} ");
            let listening = format!("pair {slot} listening");
            let lines: Vec<&str> = log
                .lines()
                .filter(|line| line.starts_with(&prefix))
                .collect();
            let took: Vec<&Decision> = decisions
                .iter()
                .filter(|decision| decision.pair == slot)
                .collect();
            let waiting = lines.iter().filter(|line| **line == listening).count();
            match took.last() {
                Some(last)
                    if last.allow
                        && lines.last() == Some(&listening.as_str())
                        && took.len() >= waiting =>
                {
                    let (run, job) = (named(last.run_id), named(last.job.as_deref()));
                    format!("{slot} busy (run {run}, job {job})")
                }
                _ => format!("{slot} idle"),
            }
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
    let _ = writeln!(text, "  pairs: {pairs}");
    let _ = writeln!(
        text,
        "  last refusal: {}",
        snapshot.refusal.as_deref().unwrap_or("none")
    );
    text
}
