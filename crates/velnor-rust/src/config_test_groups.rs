// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;
use velnor_model::{
    GroupEvent, GroupRoot, JobId, NoTests, PathGlob, RepoFacts, RepoPath, RootSlug, RustCiConfig,
    RustRoot, Shape, ShapeDecision, TestGroup, TestNeed, group_output_name,
};

use crate::config::{CONFIG_PATH, TIMEOUT_RANGE, runner};
use crate::error::{DetectError, Reason};
use crate::harness::uses_nextest;

pub(crate) const DEFAULT_NAME: &str = "default";
pub(crate) const NAME_MAX_LENGTH: usize = 40;
const FORBIDDEN_CHARACTERS: &[char] = &['"', '$', '`', '\\', '|', '&', ';', '<'];
const FORBIDDEN_WORDS: &[&str] = &["if", "for", "while", "case", "coproc"];
const THREADS_RANGE: std::ops::RangeInclusive<u16> = 1..=256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTestGroup {
    name: String,
    filter: String,
    #[serde(default)]
    needs: Vec<String>,
    test_threads: Option<u16>,
    timeout_minutes: Option<u16>,
    runner: Option<String>,
    hosted: Option<bool>,
    restore_cache: Option<bool>,
    no_tests: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    events: Vec<String>,
    root: Option<String>,
}

pub(crate) fn parse(raw: Vec<RawTestGroup>) -> Result<Vec<TestGroup>, Reason> {
    raw.into_iter().map(group).collect()
}

fn invalid(key: &'static str, value: &str) -> Reason {
    Reason::InvalidValue {
        key,
        value: value.to_owned(),
    }
}

pub(crate) fn ranged(
    key: &'static str,
    value: Option<u16>,
    range: &std::ops::RangeInclusive<u16>,
) -> Result<Option<u16>, Reason> {
    match value {
        Some(value) if !range.contains(&value) => Err(Reason::OutOfRange {
            key,
            value,
            min: *range.start(),
            max: *range.end(),
        }),
        other => Ok(other),
    }
}

fn group(raw: RawTestGroup) -> Result<TestGroup, Reason> {
    let name = JobId::new(raw.name.clone())
        .ok()
        .filter(|name| name.as_str() != DEFAULT_NAME && name.as_str().len() <= NAME_MAX_LENGTH)
        .ok_or_else(|| invalid("test_groups.name", &raw.name))?;
    if !filterset(&raw.filter) {
        return Err(Reason::TestGroupFilter(raw.filter));
    }
    let mut seen = BTreeSet::new();
    let needs = raw
        .needs
        .iter()
        .map(|need| match need.as_str() {
            "docker" => Ok(TestNeed::Docker),
            "cargo" => Ok(TestNeed::Cargo),
            other => Err(invalid("test_groups.needs", other)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(repeated) = needs.iter().find(|need| !seen.insert(**need)) {
        return Err(invalid("test_groups.needs", &format!("{repeated:?}")));
    }
    if raw.restore_cache == Some(true) && !needs.contains(&TestNeed::Cargo) {
        return Err(Reason::TestGroupRestoreWithoutCargo(raw.name));
    }
    let runner = raw
        .runner
        .map(|label| {
            runner(Some(label.clone()), RustCiConfig::default().runner)
                .map_err(|_| invalid("test_groups.runner", &label))
        })
        .transpose()?;
    let no_tests = raw
        .no_tests
        .as_deref()
        .map(|mode| match mode {
            "fail" => Ok(NoTests::Fail),
            "warn" => Ok(NoTests::Warn),
            "pass" => Ok(NoTests::Pass),
            other => Err(invalid("test_groups.no_tests", other)),
        })
        .transpose()?;
    let paths = raw
        .paths
        .iter()
        .map(|path| PathGlob::new(path.clone()).map_err(|_| invalid("test_groups.paths", path)))
        .collect::<Result<Vec<_>, _>>()?;
    let events = raw
        .events
        .iter()
        .map(|event| match event.as_str() {
            "pull_request" => Ok(GroupEvent::PullRequest),
            "push" => Ok(GroupEvent::Push),
            "merge_group" => Ok(GroupEvent::MergeGroup),
            "schedule" => Ok(GroupEvent::Schedule),
            "workflow_dispatch" => Ok(GroupEvent::WorkflowDispatch),
            other => Err(invalid("test_groups.events", other)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let root = match raw.root.as_deref() {
        None => GroupRoot::OnlyRoot,
        Some(".") => GroupRoot::Folder(None),
        Some(folder) => GroupRoot::Folder(Some(
            RepoPath::new(folder.trim_end_matches('/'))
                .map_err(|_| invalid("test_groups.root", folder))?,
        )),
    };
    Ok(TestGroup {
        name,
        filter: raw.filter,
        needs,
        test_threads: ranged("test_groups.test_threads", raw.test_threads, &THREADS_RANGE)?,
        timeout_minutes: ranged(
            "test_groups.timeout_minutes",
            raw.timeout_minutes,
            &TIMEOUT_RANGE,
        )?,
        runner,
        hosted: raw.hosted,
        restore_cache: raw.restore_cache,
        no_tests,
        paths,
        events,
        root,
    })
}

fn filterset(filter: &str) -> bool {
    !filter.trim().is_empty()
        && !filter
            .chars()
            .any(|c| c.is_control() || FORBIDDEN_CHARACTERS.contains(&c))
        && !filter
            .split_whitespace()
            .any(|word| FORBIDDEN_WORDS.contains(&word))
}

pub(crate) fn check_tests(
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
    if !config.test_groups.is_empty() && config.private_sources.is_some() {
        return Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::TestGroupsWithPrivateSources,
        ));
    }
    crate::config_test_setup::check(config, rust_roots)?;
    crate::config_check_configurations::check(config, rust_roots)?;
    check(config, rust_roots)
}

fn check(config: &RustCiConfig, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    let invalid = |reason| DetectError::invalid(&CONFIG_PATH, reason);
    let mut seen = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for group in &config.test_groups {
        let root = match &group.root {
            GroupRoot::OnlyRoot if rust_roots.len() == 1 => &rust_roots[0],
            GroupRoot::OnlyRoot => return Err(invalid(Reason::MissingKey("test_groups.root"))),
            GroupRoot::Folder(folder) => rust_roots
                .iter()
                .find(|root| root.manifest.parent() == *folder)
                .ok_or_else(|| {
                    invalid(Reason::UnknownTestGroupRoot(
                        folder
                            .as_ref()
                            .map_or_else(|| ".".to_owned(), ToString::to_string),
                    ))
                })?,
        };
        if !root
            .members
            .iter()
            .any(|member| uses_nextest(config, member))
        {
            return Err(invalid(Reason::TestGroupWithoutNextest(
                group.name.to_string(),
            )));
        }
        let duplicate_name = !seen.insert((
            group.name.as_str().to_ascii_lowercase(),
            root.manifest.clone(),
        ));
        let shared_output = !outputs.insert(group_output_name(
            &RootSlug::of(&root.manifest),
            &group.name,
        ));
        if duplicate_name || shared_output {
            return Err(invalid(Reason::InvalidValue {
                key: "test_groups.name",
                value: group.name.to_string(),
            }));
        }
    }
    Ok(())
}

pub(crate) fn of_root<'a>(
    config: &'a RustCiConfig,
    root: &'a RustRoot,
    root_count: usize,
) -> impl Iterator<Item = &'a TestGroup> {
    config
        .test_groups
        .iter()
        .filter(move |group| group.belongs_to(root, root_count))
}

pub(crate) fn archive_shape(
    facts: &RepoFacts,
    root: &RustRoot,
    decision: ShapeDecision,
) -> ShapeDecision {
    let grouped = of_root(&facts.config, root, facts.rust_roots.len())
        .next()
        .is_some();
    if grouped && decision.shape != Shape::BuildThenGroups {
        ShapeDecision {
            shape: Shape::BuildThenGroups,
            reason: format!(
                "build once, then test groups: [[test_groups]] in {CONFIG_PATH} run from one archive"
            ),
        }
    } else {
        decision
    }
}
