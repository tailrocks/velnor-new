// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use crate::pairs::Decision;
use crate::scenario::Outcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Plain,
    Pairs(Vec<Decision>),
}

impl Check {
    #[must_use]
    pub fn refused(&self) -> bool {
        match self {
            Self::Plain => false,
            Self::Pairs(decisions) => decisions.iter().any(|decision| !decision.allow),
        }
    }

    fn unadmitted(&self, outcomes: &[Outcome]) -> bool {
        match self {
            Self::Plain => false,
            Self::Pairs(decisions) => outcomes.iter().any(|outcome| {
                !decisions
                    .iter()
                    .any(|decision| decision.allow && decision.of(outcome.id, outcome.trusted))
            }),
        }
    }

    fn under(&self, outcome: &Outcome) -> Vec<String> {
        match self {
            Self::Plain => Vec::new(),
            Self::Pairs(decisions) => decisions
                .iter()
                .filter(|decision| decision.of(outcome.id, outcome.trusted))
                .map(Decision::line)
                .collect(),
        }
    }

    fn stray(&self, outcomes: &[Outcome]) -> Vec<String> {
        let Self::Pairs(decisions) = self else {
            return Vec::new();
        };
        decisions
            .iter()
            .filter(|decision| {
                !outcomes
                    .iter()
                    .any(|outcome| decision.of(outcome.id, outcome.trusted))
            })
            .map(|decision| {
                let run = decision
                    .run_id
                    .map_or_else(|| "no run".to_owned(), |id| format!("run {id}"));
                format!("  {} side, {run}: {}", decision.side, decision.line())
            })
            .collect()
    }

    fn mode(&self) -> [&'static str; 4] {
        match self {
            Self::Plain => [
                "Runner mode plain: job-start check: none (plain containers).",
                "Executed: every step of every job above except actions/checkout, by GitHub's official runner in linux/arm64 containers.",
                "Stood in: runner.server queued the jobs and replaced actions/checkout with a checkout of the local copy; the cache server decided every cache hit, miss and refusal with velnor-cache-rules; the API stub answered GITHUB_API_URL; every Ubuntu label ran on the one Ubuntu 24.04 runner image.",
                "Not exercised: velnor-host pairs, the job-start check, scale set acquire, GitHub JIT, journal, GitHub runner deletion proof.",
            ],
            Self::Pairs(_) => [
                "Runner mode pair: executed: official runner 2.338.0 in velnor-host pairs (private Docker, limits, job-start check).",
                "Stood in: jobs and job-start facts by runner.server and the driver's ledger; cache by local rules; API by the stub; actions/checkout by a checkout of the local copy.",
                "Not exercised: scale set acquire, GitHub JIT, journal, GitHub runner deletion proof.",
                "Known difference: every Ubuntu label ran on the self-hosted Ubuntu 26.04 pair image, not the hosted 24.04 one.",
            ],
        }
    }
}

pub fn conclude(check: &Check, outcomes: &[Outcome]) -> Result<(), &'static str> {
    if check.refused() {
        return Err("a job was refused at job start");
    }
    if check.unadmitted(outcomes) {
        return Err("a job has no job-start decision");
    }
    if !outcomes.iter().all(Outcome::passed) {
        return Err("a run did not succeed");
    }
    Ok(())
}

#[must_use]
pub fn verdict(
    scenario: &str,
    outcomes: &[Outcome],
    (up_at, up_seconds): (u64, f64),
    check: &Check,
    warning: &str,
) -> String {
    let mut lines = vec![format!("Verdict of scenario {scenario}")];
    for outcome in outcomes {
        lines.push(format!("  {}", outcome.line()));
        lines.extend(check.under(outcome).iter().map(|job| format!("    {job}")));
        lines.extend(outcome.caches.iter().map(|cache| format!("    {cache}")));
    }
    let stray = check.stray(outcomes);
    if !stray.is_empty() {
        lines.push("Job-start decisions outside these runs:".to_owned());
        lines.extend(stray);
    }
    let first = outcomes.iter().filter_map(|outcome| outcome.started).min();
    lines.push(format!(
        "Timing: up in {up_seconds:.1} s, first job started {} after up began.",
        first.map_or_else(
            || "never".to_owned(),
            |at| format!("{} s", at.saturating_sub(up_at))
        )
    ));
    lines.extend(check.mode().map(str::to_owned));
    lines.push(warning.to_owned());
    lines.iter().fold(String::new(), |mut text, line| {
        let _ = writeln!(text, "velnor-local {scenario}: {line}");
        text
    })
}
