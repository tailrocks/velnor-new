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
}

impl JobCondition {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "policy" => Ok(Self::Policy),
            "schedule" => Ok(Self::Schedule),
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
            Self::ScheduleOrPush => f.write_str("schedule-or-push"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictJob {
    pub id: JobId,
    pub when: Vec<JobCondition>,
    pub tests: Option<RootSlug>,
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
            let slug = tests
                .remove(&id)
                .map(RootSlug::new)
                .transpose()
                .map_err(|e| e.to_string())?;
            parsed.push(VerdictJob {
                id,
                when,
                tests: slug,
            });
        }
        if let Some(id) = tests.keys().next() {
            return Err(format!("--tests names {id}, which no --job names"));
        }
        Ok(Self { plan, jobs: parsed })
    }

    #[must_use]
    pub fn arguments(&self) -> String {
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
            if let Some(slug) = &job.tests {
                parts.push(format!("--tests {}={slug}", job.id));
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
