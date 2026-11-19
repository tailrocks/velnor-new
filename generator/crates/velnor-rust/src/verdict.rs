// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    GateNeed, JobCondition, JobId, MERGE_GROUP_SKIP_OUTPUT, RootSlug, TESTS_BY_PACKAGE_OUTPUT,
    TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT, VerdictSpec,
};

const SCHEDULE: &str = "schedule";
const PUSH: &str = "push";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobResult {
    Success,
    Failure,
    Cancelled,
    Skipped,
    Other(String),
}

impl JobResult {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        match text {
            "success" => Self::Success,
            "failure" => Self::Failure,
            "cancelled" => Self::Cancelled,
            "skipped" => Self::Skipped,
            other => Self::Other(other.to_owned()),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
            Self::Other(_) => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PackageCounts {
    pub run: u64,
    pub listed: u64,
}

/// GitHub caps one job output at 1 MB; the gate bounds the whole needs JSON separately.
pub(crate) const PACKAGE_COUNTS_LIMIT: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCounts {
    pub run: u64,
    pub listed: u64,
    pub ignored: Option<u64>,
    pub packages: BTreeMap<String, PackageCounts>,
}

#[must_use]
pub fn encode_package_counts(packages: &BTreeMap<String, PackageCounts>) -> Option<String> {
    let mut parts = Vec::with_capacity(packages.len());
    for (name, counts) in packages {
        if !package_token(name) || counts.run > counts.listed {
            return None;
        }
        parts.push(format!("{name}={}/{}", counts.run, counts.listed));
    }
    let text = parts.join(" ");
    (text.len() <= PACKAGE_COUNTS_LIMIT).then_some(text)
}

fn parse_package_counts(text: &str) -> Option<BTreeMap<String, PackageCounts>> {
    if text.len() > PACKAGE_COUNTS_LIMIT {
        return None;
    }
    let mut packages = BTreeMap::new();
    if text.is_empty() {
        return Some(packages);
    }
    for token in text.split(' ') {
        let (name, counts) = token.split_once('=')?;
        let (run, listed) = counts.split_once('/')?;
        if !package_token(name) {
            return None;
        }
        let run = number(run)?;
        let listed = number(listed)?;
        if run > listed
            || packages
                .insert(name.to_owned(), PackageCounts { run, listed })
                .is_some()
        {
            return None;
        }
    }
    let encoded = encode_package_counts(&packages)?;
    (encoded == text).then_some(packages)
}

#[must_use]
pub fn package_token(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_alphanumeric() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Need {
    pub result: Option<String>,
    pub outputs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOutput {
    pub planned: BTreeMap<JobId, bool>,
    pub test_lanes: BTreeMap<RootSlug, Vec<JobId>>,
}

impl PlanOutput {
    pub fn resolve(
        spec: &VerdictSpec,
        outputs: &BTreeMap<String, String>,
        event: Option<&str>,
    ) -> Result<Self, String> {
        let read = |key: String| -> Result<&str, String> {
            outputs
                .get(&key)
                .map(String::as_str)
                .ok_or_else(|| format!("the plan output {key} is missing"))
        };
        let flag = |key: String, on: &str, off: &str| -> Result<bool, String> {
            let value = read(key.clone())?;
            if value == on {
                Ok(true)
            } else if value == off {
                Ok(false)
            } else {
                Err(format!(
                    "the plan output {key} is `{value}`, not {on} or {off}"
                ))
            }
        };
        let mut planned = BTreeMap::new();
        let mut test_lanes: BTreeMap<RootSlug, Vec<JobId>> = BTreeMap::new();
        for job in &spec.jobs {
            let mut holds = true;
            for condition in &job.when {
                holds &= match condition {
                    JobCondition::Lane(slug) => {
                        let key = GateNeed::Selected.output_name(slug);
                        match read(key.clone())? {
                            "none" => false,
                            "some" | "all" => true,
                            other => {
                                return Err(format!(
                                    "the plan output {key} is `{other}`, not none, some or all"
                                ));
                            }
                        }
                    }
                    JobCondition::Policy => {
                        let slug = RootSlug::from_static("policy");
                        let deny = flag(GateNeed::Deny.output_name(&slug), "run", "skip")?;
                        let machete = flag(GateNeed::Machete.output_name(&slug), "run", "skip")?;
                        deny || machete
                    }
                    JobCondition::Schedule => event_is(event, &[SCHEDULE])?,
                    JobCondition::ScheduleOrDispatch => {
                        event_is(event, &[SCHEDULE, "workflow_dispatch"])?
                    }
                    JobCondition::ScheduleOrPushOrDispatch => {
                        event_is(event, &[SCHEDULE, PUSH, "workflow_dispatch"])?
                    }
                    JobCondition::ScheduleOrPush => event_is(event, &[SCHEDULE, PUSH])?,
                };
            }
            planned.insert(job.id.clone(), holds);
            if let Some(slug) = &job.tests {
                let expected = flag(GateNeed::TestsExpected.output_name(slug), "run", "skip")?;
                let jobs = test_lanes.entry(slug.clone()).or_default();
                if expected {
                    jobs.push(job.id.clone());
                }
            }
        }
        let merge_group_ran = event == Some(PUSH)
            && outputs.get(MERGE_GROUP_SKIP_OUTPUT).map(String::as_str) == Some("true");
        test_lanes.retain(|_, jobs| !merge_group_ran && !jobs.is_empty());
        Ok(Self {
            planned,
            test_lanes,
        })
    }
}

fn event_is(event: Option<&str>, names: &[&str]) -> Result<bool, String> {
    event
        .filter(|name| !name.is_empty())
        .map(|name| names.contains(&name))
        .ok_or_else(|| "GITHUB_EVENT_NAME is not set".to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub passed: bool,
    pub rows: Vec<(JobId, String, String)>,
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
}

impl Verdict {
    #[must_use]
    pub fn failed(problem: impl Into<String>) -> Self {
        Self {
            passed: false,
            rows: Vec::new(),
            problems: vec![problem.into()],
            warnings: Vec::new(),
        }
    }

    #[must_use]
    pub fn table(&self) -> String {
        let mut lines = vec![
            format!(
                "**Required: {}**",
                if self.passed { "pass" } else { "FAIL" }
            ),
            String::new(),
            "| Job | Plan | Result |".to_owned(),
            "|---|---|---|".to_owned(),
        ];
        lines.extend(
            self.rows
                .iter()
                .map(|(job, plan, result)| format!("| {job} | {plan} | {result} |")),
        );
        lines.push(String::new());
        lines.extend(self.problems.iter().map(|text| format!("- FAIL: {text}")));
        lines.extend(
            self.warnings
                .iter()
                .map(|text| format!("- warning: {text}")),
        );
        lines.join("\n") + "\n"
    }
}

#[must_use]
pub fn verdict(
    plan: &PlanOutput,
    results: &BTreeMap<JobId, JobResult>,
    tests: &BTreeMap<JobId, Option<TestCounts>>,
) -> Verdict {
    let mut out = Verdict {
        passed: true,
        rows: Vec::new(),
        problems: Vec::new(),
        warnings: Vec::new(),
    };
    for (job, scheduled) in &plan.planned {
        let result = results.get(job);
        let shown = result.map_or("missing", JobResult::name);
        out.rows.push((
            job.clone(),
            (if *scheduled {
                "scheduled"
            } else {
                "not scheduled"
            })
            .to_owned(),
            shown.to_owned(),
        ));
        match (scheduled, result) {
            (_, None) => out.problems.push(format!("{job} has no result in needs")),
            (true, Some(JobResult::Success)) | (false, Some(JobResult::Skipped)) => {}
            (true, Some(other)) => out.problems.push(format!(
                "{job} was scheduled by the plan and its result is {}",
                other.name()
            )),
            (false, Some(JobResult::Success)) => out
                .warnings
                .push(format!("{job} ran although the plan did not schedule it")),
            (false, Some(other)) => out.problems.push(format!(
                "{job} was not scheduled by the plan and its result is {}",
                other.name()
            )),
        }
    }
    for (slug, jobs) in &plan.test_lanes {
        let counts: Vec<Option<TestCounts>> = jobs
            .iter()
            .filter(|job| plan.planned.get(*job).copied().unwrap_or(false))
            .map(|job| tests.get(job).cloned().flatten())
            .collect();
        if counts.is_empty() {
            out.problems.push(format!(
                "no scheduled job reports {TESTS_BY_PACKAGE_OUTPUT} for {slug}, whose selected packages have tests"
            ));
        } else if counts.contains(&None) {
            out.problems.push(format!(
                "{TESTS_BY_PACKAGE_OUTPUT} is missing or malformed in a test job of {slug}"
            ));
        } else {
            for job in counts.iter().flatten() {
                if job.listed == 0 && job.run == 0 && job.ignored == Some(0) {
                    out.problems.push(format!(
                        "a test job of {slug} listed no tests and ran none, though its selected packages have tests"
                    ));
                }
                for (name, package) in &job.packages {
                    if package.listed > 0 && package.run == 0 {
                        out.problems.push(format!(
                            "package {name} of {slug} listed {} tests and ran none",
                            package.listed
                        ));
                    }
                }
            }
        }
    }
    out.passed = out.problems.is_empty();
    out
}

#[must_use]
pub fn decide(spec: &VerdictSpec, needs: &BTreeMap<JobId, Need>, event: Option<&str>) -> Verdict {
    let Some(plan_job) = needs.get(&spec.plan) else {
        return Verdict::failed(format!("the plan job {} is not in needs", spec.plan));
    };
    if plan_job.result.as_deref().map(JobResult::parse) != Some(JobResult::Success) {
        return Verdict::failed(format!(
            "the plan job {} did not succeed (result: {})",
            spec.plan,
            plan_job.result.as_deref().unwrap_or("missing")
        ));
    }
    let plan = match PlanOutput::resolve(spec, &plan_job.outputs, event) {
        Ok(plan) => plan,
        Err(problem) => return Verdict::failed(problem),
    };
    let results = needs
        .iter()
        .filter_map(|(id, need)| Some((id.clone(), JobResult::parse(need.result.as_deref()?))))
        .collect();
    let tests = spec
        .jobs
        .iter()
        .filter(|job| job.tests.is_some())
        .map(|job| (job.id.clone(), counts(needs.get(&job.id))))
        .collect();
    let mut out = verdict(&plan, &results, &tests);
    for id in needs.keys().filter(|id| !plan.planned.contains_key(*id)) {
        out.problems
            .push(format!("{id} is in needs and the gate does not know it"));
    }
    out.passed = out.problems.is_empty();
    out
}

fn counts(need: Option<&Need>) -> Option<TestCounts> {
    let need = need?;
    let packages = parse_package_counts(need.outputs.get(TESTS_BY_PACKAGE_OUTPUT)?)?;
    let run = number(need.outputs.get(TESTS_RUN_OUTPUT)?)?;
    let listed = number(need.outputs.get(TESTS_LISTED_OUTPUT)?)?;
    let summed_run = packages
        .values()
        .try_fold(0u64, |total, package| total.checked_add(package.run))?;
    let summed_listed = packages
        .values()
        .try_fold(0u64, |total, package| total.checked_add(package.listed))?;
    if run != summed_run || listed != summed_listed {
        return None;
    }
    let ignored = match need.outputs.get(TESTS_IGNORED_OUTPUT).map(String::as_str) {
        None | Some("") => None,
        Some(text) => Some(number(text)?),
    };
    Some(TestCounts {
        run,
        listed,
        ignored,
        packages,
    })
}

fn number(text: &str) -> Option<u64> {
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    text.parse().ok()
}

#[cfg(test)]
#[path = "verdict_package_map.rs"]
mod package_map_size;
