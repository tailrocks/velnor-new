// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod yaml;

use std::collections::BTreeMap;

use velnor_model::{
    Access, CargoCommand, Job, PermissionScope, Permissions, RepoPath, Step, StepKind, Triggers,
    WorkflowPlan,
};

use crate::yaml::{Yaml, emit};

pub const WORKFLOW_PATH: RepoPath = RepoPath::from_static(".github/workflows/rust-ci.yml");
pub const OWNED_PATHS: [RepoPath; 1] = [WORKFLOW_PATH];

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

#[must_use]
pub fn render(plan: &WorkflowPlan) -> BTreeMap<RepoPath, String> {
    let workflow = [
        entry("name", text(&plan.name)),
        entry("on", triggers(&plan.triggers)),
        entry("permissions", permissions(&plan.permissions)),
        entry(
            "concurrency",
            Yaml::Map(vec![
                entry("group", Yaml::Expr("${{ github.workflow_ref }}".to_owned())),
                entry(
                    "cancel-in-progress",
                    Yaml::Bool(plan.concurrency.cancel_in_progress),
                ),
            ]),
        ),
        entry(
            "jobs",
            Yaml::Map(
                plan.jobs
                    .iter()
                    .map(|job| (job.id.to_string(), render_job(job)))
                    .collect(),
            ),
        ),
    ];
    BTreeMap::from([(WORKFLOW_PATH, emit(&workflow))])
}

fn triggers(triggers: &Triggers) -> Yaml {
    let mut items = Vec::new();
    if triggers.pull_request {
        items.push(entry("pull_request", Yaml::Map(Vec::new())));
    }
    if triggers.workflow_dispatch {
        items.push(entry("workflow_dispatch", Yaml::Map(Vec::new())));
    }
    Yaml::Map(items)
}

fn permissions(permissions: &Permissions) -> Yaml {
    Yaml::Map(
        permissions
            .0
            .iter()
            .map(|(scope, access)| {
                let scope = match scope {
                    PermissionScope::Actions => "actions",
                    PermissionScope::Contents => "contents",
                    PermissionScope::PullRequests => "pull-requests",
                };
                let access = match access {
                    Access::None => "none",
                    Access::Read => "read",
                    Access::Write => "write",
                };
                entry(scope, text(access))
            })
            .collect(),
    )
}

fn render_job(job: &Job) -> Yaml {
    Yaml::Map(vec![
        entry("runs-on", text(job.runs_on.as_str())),
        entry("timeout-minutes", Yaml::Int(u64::from(job.timeout_minutes))),
        entry(
            "steps",
            Yaml::Seq(job.steps.iter().map(render_step).collect()),
        ),
    ])
}

fn render_step(step: &Step) -> Yaml {
    match &step.kind {
        StepKind::Checkout { depth } => Yaml::Map(vec![
            entry("name", text("Check out")),
            entry("uses", text(CHECKOUT)),
            entry(
                "with",
                Yaml::Map(vec![
                    entry("fetch-depth", Yaml::Int(u64::from(*depth))),
                    entry("persist-credentials", Yaml::Bool(false)),
                ]),
            ),
        ]),
        StepKind::Cargo(CargoCommand::FmtCheck { manifest }) => {
            let mut items = vec![entry("name", Yaml::Str(format!("Format {manifest}")))];
            if let Some(directory) = manifest.parent() {
                items.push(entry("working-directory", text(directory.as_str())));
            }
            items.push(entry("run", text("cargo-fmt --all --check")));
            Yaml::Map(items)
        }
    }
}

fn entry(key: &str, value: Yaml) -> (String, Yaml) {
    (key.to_owned(), value)
}

fn text(value: &str) -> Yaml {
    Yaml::Str(value.to_owned())
}
