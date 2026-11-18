// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;

use crate::ids::{JobId, RootSlug};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobCondition {
    Lane(RootSlug),
    Policy,
    Schedule,
    ScheduleOrPush,
    ScheduleOrDispatch,
    ScheduleOrPushOrDispatch,
}

impl JobCondition {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "policy" => Ok(Self::Policy),
            "schedule" => Ok(Self::Schedule),
            "schedule-or-dispatch" => Ok(Self::ScheduleOrDispatch),
            "schedule-or-push-or-dispatch" => Ok(Self::ScheduleOrPushOrDispatch),
            "schedule-or-push" => Ok(Self::ScheduleOrPush),
            other => other
                .strip_prefix("lane:")
                .and_then(|slug| RootSlug::new(slug).ok())
                .map(Self::Lane)
                .ok_or_else(|| format!("`{other}` is not a job condition")),
        }
    }
}

impl fmt::Display for JobCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lane(slug) => write!(f, "lane:{slug}"),
            Self::Policy => f.write_str("policy"),
            Self::Schedule => f.write_str("schedule"),
            Self::ScheduleOrDispatch => f.write_str("schedule-or-dispatch"),
            Self::ScheduleOrPushOrDispatch => f.write_str("schedule-or-push-or-dispatch"),
            Self::ScheduleOrPush => f.write_str("schedule-or-push"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictJob {
    pub id: JobId,
    pub when: Vec<JobCondition>,
    pub tests: Vec<RootSlug>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictSpec {
    pub plan: JobId,
    pub jobs: Vec<VerdictJob>,
}

const ALWAYS: &str = "always";
const JOIN: char = '+';

impl VerdictSpec {
    pub fn parse(plan: &str, jobs: &[String], tests: &[String]) -> Result<Self, String> {
        let plan = JobId::new(plan).map_err(|e| e.to_string())?;
        let mut tests = pairs(tests, "--tests")?;
        let mut parsed: Vec<VerdictJob> = Vec::new();
        for (id, text) in pairs(jobs, "--job")? {
            let when = if text == ALWAYS {
                Vec::new()
            } else {
                text.split(JOIN)
                    .map(JobCondition::parse)
                    .collect::<Result<_, _>>()?
            };
            let slugs = tests
                .remove(&id)
                .map(|roots| {
                    roots
                        .split(',')
                        .map(RootSlug::new)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| e.to_string())
                })
                .transpose()?
                .unwrap_or_default();
            parsed.push(VerdictJob {
                id,
                when,
                tests: slugs,
            });
        }
        if let Some(id) = tests.keys().next() {
            return Err(format!("--tests names {id}, which no --job names"));
        }
        Ok(Self { plan, jobs: parsed })
    }

    pub fn from_text(text: &str) -> Result<Self, String> {
        let mut plan = None;
        let mut jobs = Vec::new();
        let mut tests = Vec::new();
        let mut words = text.split_whitespace();
        while let Some(flag) = words.next() {
            let value = words
                .next()
                .ok_or_else(|| format!("the gate spec ends after {flag}"))?;
            match flag {
                "--plan" if plan.is_none() => plan = Some(value.to_owned()),
                "--plan" => return Err("the gate spec names --plan twice".to_owned()),
                "--job" => jobs.push(value.to_owned()),
                "--tests" => tests.push(value.to_owned()),
                other => return Err(format!("the gate spec holds `{other}`")),
            }
        }
        let plan = plan.ok_or("the gate spec has no --plan")?;
        Self::parse(&plan, &jobs, &tests)
    }

    #[must_use]
    pub fn text(&self) -> String {
        let mut parts = vec![format!("--plan {}", self.plan)];
        for job in &self.jobs {
            let when = if job.when.is_empty() {
                ALWAYS.to_owned()
            } else {
                job.when
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(&JOIN.to_string())
            };
            parts.push(format!("--job {}={when}", job.id));
        }
        for job in &self.jobs {
            if !job.tests.is_empty() {
                let slugs = job
                    .tests
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                parts.push(format!("--tests {}={slugs}", job.id));
            }
        }
        parts.join(" ")
    }
}

fn pairs(items: &[String], flag: &str) -> Result<BTreeMap<JobId, String>, String> {
    let mut map = BTreeMap::new();
    for item in items {
        let (id, value) = item
            .split_once('=')
            .ok_or_else(|| format!("{flag} expects ID=VALUE, not `{item}`"))?;
        let id = JobId::new(id).map_err(|e| e.to_string())?;
        if map.insert(id.clone(), value.to_owned()).is_some() {
            return Err(format!("{flag} names {id} twice"));
        }
    }
    Ok(map)
}
