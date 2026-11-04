// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use velnor_actions_contract::workflow::ProducerKind;
use velnor_actions_contract::{Job, PLAN_JOB_ID};

use crate::tools_identity::tools_digest;
use crate::waste_lint::WasteFinding;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolsIdentityFinding {
    Split {
        runs_on: String,
        target: String,
        jobs: Vec<(String, String)>,
    },
    Unreadable {
        job: String,
        problem: String,
    },
}

impl Display for ToolsIdentityFinding {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Split {
                runs_on,
                target,
                jobs,
            } => {
                let listed: Vec<String> = jobs
                    .iter()
                    .map(|(job, digest)| format!("{job} ({digest})"))
                    .collect();
                write!(
                    f,
                    "runs_on {runs_on} target {target} installs rust under {} tools identities: {}",
                    jobs.iter()
                        .map(|(_, digest)| digest)
                        .collect::<BTreeSet<_>>()
                        .len(),
                    listed.join(", ")
                )
            }
            Self::Unreadable { job, problem } => {
                write!(f, "job {job} has an unreadable tools identity: {problem}")
            }
        }
    }
}

fn in_scope(id: &str, job: &Job) -> bool {
    id != PLAN_JOB_ID
        && matches!(
            job.producer_kind(),
            None | Some(ProducerKind::StateProducer)
        )
        && crate::rust_job_env::installs_rust(job)
}

pub(crate) fn w7(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    let mut findings = Vec::new();
    let mut lanes: BTreeMap<(&str, &str), Vec<(String, String)>> = BTreeMap::new();
    for (id, job) in jobs.iter().filter(|(id, job)| in_scope(id, job)) {
        let digest = match tools_digest(id, job) {
            Ok(Some(digest)) => digest,
            Ok(None) => continue,
            Err(err) => {
                findings.push(WasteFinding::ToolsIdentity(
                    ToolsIdentityFinding::Unreadable {
                        job: id.clone(),
                        problem: err.to_string(),
                    },
                ));
                continue;
            }
        };
        let Some(target) = crate::runs_on::lane_target(job) else {
            continue;
        };
        lanes
            .entry((job.runs_on.as_str(), target))
            .or_default()
            .push((id.clone(), digest.short().to_owned()));
    }
    findings.extend(lanes.into_iter().filter_map(|((runs_on, target), jobs)| {
        let distinct: BTreeSet<&String> = jobs.iter().map(|(_, digest)| digest).collect();
        (distinct.len() > 1).then(|| {
            WasteFinding::ToolsIdentity(ToolsIdentityFinding::Split {
                runs_on: runs_on.to_owned(),
                target: target.to_owned(),
                jobs,
            })
        })
    }));
    findings
}
