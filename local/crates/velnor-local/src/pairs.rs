// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use serde_json::{Value, json};

use crate::side::Side;

pub const DECISIONS: &str = "decisions.jsonl";
pub const MARKER: &str = "velnor-local";
pub const DEFAULT_RUNNER_CPU_MILLICORES: u64 = 4000;
pub const DEFAULT_RUNNER_MEMORY_BYTES: u64 = 6 << 30;
const DIND_CPU_MILLICORES: u64 = 2000;
const DIND_MEMORY_BYTES: u64 = 2 << 30;
const MIN_RUNNER_MEMORY_BYTES: u64 = 512 << 20;
const MAX_RUNNER_MILLICORES: u64 = 1_024_000;
pub const DEFAULT_HOME_BYTES: u64 = 2 << 30;
pub const DEFAULT_WORK_BYTES: u64 = 16 << 30;
pub const DEFAULT_TMP_BYTES: u64 = 2 << 30;
const MIN_STORAGE_BYTES: u64 = 1 << 30;
const MAX_STORAGE_BYTES: u64 = 64 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    pub home_bytes: u64,
    pub work_bytes: u64,
    pub tmp_bytes: u64,
}

impl Default for Sizes {
    fn default() -> Self {
        Self {
            home_bytes: DEFAULT_HOME_BYTES,
            work_bytes: DEFAULT_WORK_BYTES,
            tmp_bytes: DEFAULT_TMP_BYTES,
        }
    }
}

/// What one pair may use: the runner's share is set by flags; the dind's is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub runner_cpu_millicores: u64,
    pub runner_memory_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            runner_cpu_millicores: DEFAULT_RUNNER_CPU_MILLICORES,
            runner_memory_bytes: DEFAULT_RUNNER_MEMORY_BYTES,
        }
    }
}

impl Limits {
    /// Memory that one pair, runner plus dind, may take together.
    #[must_use]
    pub const fn pair_memory_bytes(&self) -> u64 {
        self.runner_memory_bytes + DIND_MEMORY_BYTES
    }
}

pub fn parse_size(flag: &str, text: &str, min: u64, max: Option<u64>) -> Result<u64, String> {
    let bad = |why: &str| {
        format!("{flag} {text:?}: {why}; use a whole number with m or g, such as 512m or 6g")
    };
    let amount = |bytes: u64| {
        if bytes.is_multiple_of(1 << 30) {
            format!("{}g", bytes >> 30)
        } else {
            format!("{}m", bytes >> 20)
        }
    };
    let text = text.trim();
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    let (number, unit) = text.split_at(digits);
    let unit = match unit.to_ascii_lowercase().as_str() {
        "m" | "mib" => 20,
        "g" | "gib" => 30,
        _ => return Err(bad("the unit is missing or unknown")),
    };
    let bytes = number
        .parse::<u64>()
        .ok()
        .and_then(|count| count.checked_mul(1 << unit))
        .ok_or_else(|| bad("the number is missing or too large"))?;
    if bytes < min {
        return Err(bad(&format!("the least is {}", amount(min))));
    }
    if let Some(max) = max.filter(|max| bytes > *max) {
        return Err(bad(&format!("at most {}", amount(max))));
    }
    Ok(bytes)
}

pub fn parse_memory(text: &str) -> Result<u64, String> {
    parse_size("--runner-memory", text, MIN_RUNNER_MEMORY_BYTES, None)
}

pub fn parse_home_size(text: &str) -> Result<u64, String> {
    parse_size(
        "--home-size",
        text,
        MIN_STORAGE_BYTES,
        Some(MAX_STORAGE_BYTES),
    )
}

pub fn parse_work_size(text: &str) -> Result<u64, String> {
    parse_size(
        "--work-size",
        text,
        MIN_STORAGE_BYTES,
        Some(MAX_STORAGE_BYTES),
    )
}

pub fn parse_tmp_size(text: &str) -> Result<u64, String> {
    parse_size(
        "--tmp-size",
        text,
        MIN_STORAGE_BYTES,
        Some(MAX_STORAGE_BYTES),
    )
}

/// Parses `--runner-cpus`: a positive decimal with up to three places, returned in millicores.
pub fn parse_cpus(text: &str) -> Result<u64, String> {
    let bad = |why: &str| {
        format!("--runner-cpus {text:?}: {why}; use a positive number such as 4 or 1.5")
    };
    let (whole, fraction) = text.trim().split_once('.').unwrap_or((text.trim(), ""));
    let numeric = |part: &str| part.chars().all(|c| c.is_ascii_digit());
    if !numeric(whole) || !numeric(fraction) || (whole.is_empty() && fraction.is_empty()) {
        return Err(bad("not a decimal number"));
    }
    if fraction.len() > 3 {
        return Err(bad("at most three decimal places"));
    }
    let places = format!("{fraction:0<3}");
    let milli = format!("{}{places}", if whole.is_empty() { "0" } else { whole })
        .parse::<u64>()
        .map_err(|_| bad("too large"))?;
    match milli {
        0 => Err(bad("must be above zero")),
        m if m > MAX_RUNNER_MILLICORES => Err(bad("at most 1024")),
        m => Ok(m),
    }
}

#[derive(Debug, Default)]
pub struct PlanInput<'a> {
    pub environment: &'a str,
    pub authority: &'a str,
    pub token: &'a str,
    pub docker: &'a str,
    pub runner_image: &'a str,
    pub dind_image: &'a str,
    pub sides: &'a [(Side, String, u16)],
    pub policy: Option<&'a str>,
    pub limits: Limits,
    pub storage: Sizes,
    pub local_ci: Option<&'a str>,
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
    let mut planned = json!({
        "environment": input.environment,
        "authority": format!("http://{}", input.authority),
        "token": input.token,
        "docker": input.docker,
        "platform": "linux/arm64",
        "runner_image": input.runner_image,
        "dind_image": input.dind_image,
        "limits": {
            "runner_cpu_millicores": input.limits.runner_cpu_millicores,
            "runner_memory_bytes": input.limits.runner_memory_bytes,
            "dind_cpu_millicores": DIND_CPU_MILLICORES,
            "dind_memory_bytes": DIND_MEMORY_BYTES,
        },
        "storage": {
            "home_bytes": input.storage.home_bytes,
            "work_bytes": input.storage.work_bytes,
            "tmp_bytes": input.storage.tmp_bytes,
        },
        "sides": sides,
        "policy": input.policy,
    });
    if let Some(binary) = input.local_ci {
        planned["local_ci"] = json!(binary);
    }
    planned
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

#[must_use]
pub fn refused_among(all: &[Decision], trusted: bool, runs: &[u64]) -> bool {
    all.iter()
        .any(|line| !line.allow && runs.iter().any(|run| line.of(*run, trusted)))
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
