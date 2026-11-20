// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CratesMirror, HostedRunners, MisePlatform, RunnerLabel, TestGroup, TrustedRunners,
    looks_github_hosted,
};

use crate::config_fork::expression_safe;
use crate::config_mirror::RawCratesMirror;
use crate::error::Reason;

pub(crate) struct Fixed<'a> {
    runner: &'a RunnerLabel,
    fork_runner: &'a RunnerLabel,
    hosted_runners: &'a HostedRunners,
    mirror_runner: Option<&'a RunnerLabel>,
    groups: &'a [TestGroup],
}

pub(crate) struct Declared<'a> {
    pub(crate) runner: &'a RunnerLabel,
    pub(crate) explicit_runner: bool,
    pub(crate) hosted: Option<bool>,
    pub(crate) fork_runner: Option<String>,
    pub(crate) trusted_runner: Option<String>,
    pub(crate) crates_mirror: Option<RawCratesMirror>,
    pub(crate) test_groups: &'a [TestGroup],
}

pub(crate) struct Runners {
    pub(crate) crates_mirror: Option<CratesMirror>,
    pub(crate) hosted: HostedRunners,
    pub(crate) fork: RunnerLabel,
    pub(crate) trusted: TrustedRunners,
}

impl Declared<'_> {
    pub(crate) fn resolve(self) -> Result<Runners, Reason> {
        let mirror_key = self
            .crates_mirror
            .as_ref()
            .and_then(|mirror| mirror.trusted_runner.clone());
        let crates_mirror = crate::config_mirror::optional(self.crates_mirror)?;
        let hosted_key =
            self.hosted.is_some() || self.test_groups.iter().any(|group| group.hosted.is_some());
        let hosted_runners = crate::config_hosting::resolve(
            (
                self.runner,
                self.hosted.or((!self.explicit_runner).then_some(true)),
            ),
            crates_mirror.as_ref(),
            self.test_groups,
        )?;
        let fork_runner = crate::config_fork::fork_runner(self.fork_runner, self.runner)?;
        let trusted_runners = resolve(
            (self.trusted_runner, mirror_key),
            (self.runner, &fork_runner, &hosted_runners),
            crates_mirror.as_ref(),
            self.test_groups,
        )?;
        Ok(Runners {
            crates_mirror,
            hosted: hosted_runners.with_key_written(hosted_key),
            fork: fork_runner,
            trusted: trusted_runners,
        })
    }
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
    .chain(
        fixed
            .groups
            .iter()
            .filter_map(|group| group.runner.as_ref()),
    )
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

pub(crate) fn resolve(
    keys: (Option<String>, Option<String>),
    runners: (&RunnerLabel, &RunnerLabel, &HostedRunners),
    mirror: Option<&CratesMirror>,
    groups: &[TestGroup],
) -> Result<TrustedRunners, Reason> {
    let (trusted_runner, mirror_trusted_runner) = keys;
    let (runner, fork_runner, hosted_runners) = runners;
    let fixed = Fixed {
        runner,
        fork_runner,
        hosted_runners,
        mirror_runner: mirror.map(|mirror| &mirror.runner),
        groups,
    };
    let top = trusted_runner
        .as_deref()
        .map(|configured| label("trusted_runner", configured, runner, &fixed))
        .transpose()?;
    let mut pairs: Vec<(RunnerLabel, RunnerLabel)> = top
        .iter()
        .map(|trusted| (runner.clone(), trusted.clone()))
        .collect();
    let Some(mirror_runner) = fixed.mirror_runner else {
        return Ok(TrustedRunners::new(pairs));
    };
    let own = mirror_trusted_runner
        .as_deref()
        .map(|configured| {
            label(
                "crates_mirror.trusted_runner",
                configured,
                mirror_runner,
                &fixed,
            )
        })
        .transpose()?;
    let shares_runner = same(mirror_runner, runner);
    let effective = own.or_else(|| shares_runner.then(|| top.clone()).flatten());
    if shares_runner && effective != top {
        return Err(Reason::InvalidValue {
            key: "crates_mirror.trusted_runner",
            value: mirror_trusted_runner.unwrap_or_default(),
        });
    }
    if let (false, Some(trusted)) = (shares_runner, effective) {
        pairs.push((mirror_runner.clone(), trusted));
    }
    Ok(TrustedRunners::new(pairs))
}
