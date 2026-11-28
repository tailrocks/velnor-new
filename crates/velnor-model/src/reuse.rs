// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::JobId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReuseStep {
    Record,
    Upload,
    Decide { required: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reuse {
    pub id: String,
    pub plan: JobId,
}

impl Reuse {
    #[must_use]
    pub fn job(job: &JobId, plan: &JobId) -> Self {
        Self {
            id: job.to_string(),
            plan: plan.clone(),
        }
    }

    #[must_use]
    pub fn step(job: &JobId, kind: &str, position: usize, plan: &JobId) -> Self {
        Self {
            id: format!("{job}.{kind}-{position}"),
            plan: plan.clone(),
        }
    }
}

#[must_use]
pub fn no_run<S: AsRef<str>>(command: &[S]) -> Vec<String> {
    let end = command
        .iter()
        .position(|word| word.as_ref() == "--")
        .unwrap_or(command.len());
    let mut built: Vec<String> = command[..end]
        .iter()
        .map(AsRef::as_ref)
        .filter(|word| {
            !matches!(*word, "--no-fail-fast" | "--fail-fast") && !word.starts_with("--no-tests=")
        })
        .map(str::to_owned)
        .collect();
    built.push("--no-run".to_owned());
    built.extend(command[end..].iter().map(|word| word.as_ref().to_owned()));
    built
}
