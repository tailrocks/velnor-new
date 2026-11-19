// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{MisePlatform, RunnerLabel, github_hosted};

use crate::error::Reason;

fn lock_platform(runner: &RunnerLabel) -> Option<String> {
    MisePlatform::runner(runner.as_str()).map(MisePlatform::lock_key)
}

pub(crate) fn fork_runner(
    configured: Option<String>,
    runner: &RunnerLabel,
) -> Result<RunnerLabel, Reason> {
    let platform = lock_platform(runner);
    let Some(label) = configured else {
        return Ok(RunnerLabel::from_static(match platform.as_deref() {
            Some("linux-arm64") => "ubuntu-24.04-arm",
            Some("macos-arm64") => "macos-15",
            _ => "ubuntu-latest",
        }));
    };
    let invalid = || Reason::InvalidValue {
        key: "fork_runner",
        value: label.clone(),
    };
    let fork = RunnerLabel::new(label.clone()).map_err(|_| invalid())?;
    let compatible = match platform {
        Some(primary) => lock_platform(&fork).as_ref() == Some(&primary),
        None => lock_platform(&fork).as_deref() == Some("linux-x64"),
    };
    if github_hosted(fork.as_str()) && compatible {
        Ok(fork)
    } else {
        Err(invalid())
    }
}

pub(crate) fn expression_safe(runner: &RunnerLabel) -> bool {
    github_hosted(runner.as_str())
        || runner
            .as_str()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}
