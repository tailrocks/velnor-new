// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use serde_json::{Value, json};

use crate::side::Side;

pub const DECISIONS: &str = "decisions.jsonl";
pub const MARKER: &str = "velnor-local";
const RUNNER_CPU_MILLICORES: u64 = 4000;
const RUNNER_MEMORY_BYTES: u64 = 6 << 30;
const DIND_CPU_MILLICORES: u64 = 2000;
const DIND_MEMORY_BYTES: u64 = 2 << 30;

#[derive(Debug)]
pub struct PlanInput<'a> {
    pub environment: &'a str,
    pub authority: &'a str,
    pub token: &'a str,
    pub docker: &'a str,
    pub runner_image: &'a str,
    pub dind_image: &'a str,
    pub sides: &'a [(Side, String, u16)],
}

#[must_use]
pub fn plan(input: &PlanInput<'_>) -> Value {
    let sides: Vec<Value> = input
        .sides
        .iter()
        .filter(|(_, _, pairs)| *pairs > 0)
        .map(|(side, network, pairs)| {
            json!({"side": side.name(), "network": network, "pairs": pairs})
        })
        .collect();
    json!({
        "environment": input.environment,
        "authority": format!("http://{}", input.authority),
        "token": input.token,
        "docker": input.docker,
        "platform": "linux/arm64",
        "runner_image": input.runner_image,
        "dind_image": input.dind_image,
        "limits": {
            "runner_cpu_millicores": RUNNER_CPU_MILLICORES,
            "runner_memory_bytes": RUNNER_MEMORY_BYTES,
            "dind_cpu_millicores": DIND_CPU_MILLICORES,
            "dind_memory_bytes": DIND_MEMORY_BYTES,
        },
        "sides": sides,
    })
}

#[must_use]
pub fn slots(trusted: u16, untrusted: u16) -> Vec<String> {
    let side = |side: Side, pairs: u16| (1..=pairs).map(move |n| format!("{}-{n}", side.name()));
    side(Side::Trusted, trusted)
        .chain(side(Side::Untrusted, untrusted))
        .collect()
}

#[must_use]
pub fn listening<'a>(log: &str, slots: &'a [String]) -> Vec<&'a str> {
    slots
        .iter()
        .filter(|slot| {
            let prefix = format!("pair {slot} ");
            log.lines()
                .rfind(|line| line.starts_with(&prefix))
                .is_some_and(|line| line == format!("pair {slot} listening"))
        })
        .map(String::as_str)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Decision {
    pub pair: String,
    pub side: String,
    pub allow: bool,
    pub reason: Option<String>,
    pub ledger: Option<String>,
    pub run_id: Option<u64>,
    pub job: Option<String>,
}

impl Decision {
    #[must_use]
    pub fn line(&self) -> String {
        let job = self.job.as_deref().unwrap_or("unknown");
        let judged = if self.allow {
            let ledger = self.ledger.as_deref().unwrap_or("none");
            format!("admitted (event {ledger})")
        } else {
            let reason = self.reason.as_deref().unwrap_or("no reason given");
            format!("REFUSED at job start: {reason}")
        };
        format!("job {job}: pair {}, {judged}", self.pair)
    }

    #[must_use]
    pub fn of(&self, run: u64, trusted: bool) -> bool {
        let side = if trusted {
            Side::Trusted
        } else {
            Side::Untrusted
        };
        self.run_id == Some(run) && self.side == side.name()
    }
}

pub fn decisions(text: &str) -> Result<Vec<Decision>, String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|error| format!("{DECISIONS} holds an unreadable line {line}: {error}"))
        })
        .collect()
}
