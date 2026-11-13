// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub description: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Push(String),
    Commit {
        branch: String,
        append: String,
        line: String,
    },
    OpenPullRequest {
        head: String,
        base: String,
    },
}

impl Scenario {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("not a scenario: {e}"))
    }
}

#[derive(Debug)]
pub struct Outcome {
    pub id: u64,
    pub workflow: String,
    pub event: String,
    pub conclusion: &'static str,
    pub started: Option<u64>,
    pub metrics: Result<PathBuf, String>,
}

impl Outcome {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.conclusion == "success" && self.metrics.is_ok()
    }

    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "run {} {} ({}): {}",
            self.id, self.workflow, self.event, self.conclusion
        )
    }
}

#[must_use]
pub fn verdict(scenario: &str, outcomes: &[Outcome], up_at: u64, up_seconds: f64) -> String {
    let mut text = format!("\nVerdict of scenario {scenario}\n");
    for outcome in outcomes {
        let _ = writeln!(text, "  {}", outcome.line());
    }
    let first = outcomes.iter().filter_map(|outcome| outcome.started).min();
    let _ = writeln!(
        text,
        "Timing: up in {up_seconds:.1} s, first job started {} after up began.",
        first.map_or_else(
            || "never".to_owned(),
            |at| format!("{} s", at.saturating_sub(up_at))
        )
    );
    text.push_str(
        "Executed: every step of every job above, by GitHub's official runner in linux/arm64 containers.\n\
         Stood in: runner.server queued the jobs and checked out the local copy of the repository; \
         the cache server decided every cache hit, miss and refusal with velnor-cache-rules; \
         the API stub answered GITHUB_API_URL.\n",
    );
    text
}
