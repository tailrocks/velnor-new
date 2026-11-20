// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HostedRunners, MisePlatform, RunnerLabel, TrustedRunners, looks_github_hosted};

use crate::config_fork::expression_safe;
use crate::error::Reason;

pub(crate) struct Raw {
    pub(crate) trusted_runner: Option<String>,
    pub(crate) mirror_trusted_runner: Option<String>,
}

pub(crate) struct Fixed<'a> {
    pub(crate) runner: &'a RunnerLabel,
    pub(crate) fork_runner: &'a RunnerLabel,
    pub(crate) hosted_runners: &'a HostedRunners,
    pub(crate) mirror_runner: Option<&'a RunnerLabel>,
}

fn lock_platform(label: &RunnerLabel) -> Option<String> {
    MisePlatform::runner(label.as_str()).map(MisePlatform::lock_key)
}

fn same(left: &RunnerLabel, right: &RunnerLabel) -> bool {
    left.as_str().eq_ignore_ascii_case(right.as_str())
}

fn label(
    key: &'static str,
    configured: &str,
    primary: &RunnerLabel,
    fixed: &Fixed<'_>,
) -> Result<RunnerLabel, Reason> {
    let invalid = || Reason::InvalidValue {
        key,
        value: configured.to_owned(),
    };
    let trusted = RunnerLabel::new(configured.to_owned()).map_err(|_| invalid())?;
    let distinct = [
        Some(fixed.runner),
        Some(fixed.fork_runner),
        fixed.mirror_runner,
    ]
    .into_iter()
    .flatten()
    .all(|other| !same(&trusted, other));
    let acceptable = expression_safe(&trusted)
        && !looks_github_hosted(trusted.as_str())
        && !fixed.hosted_runners.contains(primary)
        && distinct
        && lock_platform(&trusted) == lock_platform(primary);
    if acceptable {
        Ok(trusted)
    } else {
        Err(invalid())
    }
}

pub(crate) fn resolve(raw: Raw, fixed: &Fixed<'_>) -> Result<TrustedRunners, Reason> {
    let top = raw
        .trusted_runner
        .as_deref()
        .map(|configured| label("trusted_runner", configured, fixed.runner, fixed))
        .transpose()?;
    let mut pairs: Vec<(RunnerLabel, RunnerLabel)> = top
        .iter()
        .map(|trusted| (fixed.runner.clone(), trusted.clone()))
        .collect();
    let Some(mirror_runner) = fixed.mirror_runner else {
        return Ok(TrustedRunners::new(pairs));
    };
    let own = raw
        .mirror_trusted_runner
        .as_deref()
        .map(|configured| {
            label(
                "crates_mirror.trusted_runner",
                configured,
                mirror_runner,
                fixed,
            )
        })
        .transpose()?;
    let shares_runner = same(mirror_runner, fixed.runner);
    let effective = own.or_else(|| shares_runner.then(|| top.clone()).flatten());
    if shares_runner && effective != top {
        return Err(Reason::InvalidValue {
            key: "crates_mirror.trusted_runner",
            value: raw.mirror_trusted_runner.unwrap_or_default(),
        });
    }
    if let (false, Some(trusted)) = (shares_runner, effective) {
        pairs.push((mirror_runner.clone(), trusted));
    }
    Ok(TrustedRunners::new(pairs))
}
