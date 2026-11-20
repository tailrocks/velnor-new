// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::GENERATED_HEADER;
use crate::release::LINT_IGNORE;
use crate::rules::{RenderedJob, RenderedStep, Value};
use crate::steps::install_helper;
use crate::workflow::{ShellcheckMarks, yaml_job};
use crate::yaml::{Yaml, emit, entry, text};
use velnor_model::{CACHE_TRIM_WORKFLOW_PATH, HelperRelease, RepoPath};

pub const DELTA_CLEANUP_PATH: RepoPath = CACHE_TRIM_WORKFLOW_PATH;

#[must_use]
pub fn delta_cleanup(helper: &HelperRelease, workflow: &str) -> Option<String> {
    // helper-gate: first in tools-v0.1.102 via d56256c182
    if !helper.version().at_least((0, 1, 102)) {
        return None;
    }
    let job = RenderedJob {
        id: "rust-trim".to_owned(),
        condition: Some("${{ github.event.workflow_run.event == 'pull_request' && github.event.workflow_run.path == '.github/workflows/rust-ci.yml' && github.event.workflow_run.head_repository.id == github.event.repository.id }}".to_owned()),
        runs_on: "ubuntu-latest".to_owned(),
        timeout_minutes: 5,
        permissions: vec![
            ("actions".to_owned(), "write".to_owned()),
            ("contents".to_owned(), "read".to_owned()),
        ],
        steps: vec![
            install_helper(helper),
            RenderedStep {
                name: "Delete older PR delta entries".to_owned(),
                env: vec![
                    (
                        "GITHUB_TOKEN".to_owned(),
                        Value::Expr("${{ github.token }}".to_owned()),
                    ),
                    (
                        "RUN_ID".to_owned(),
                        Value::Expr("${{ github.event.workflow_run.id }}".to_owned()),
                    ),
                ],
                run: Some("velnor-ci cache trim-deltas --run-id \"$RUN_ID\"".to_owned()),
                ..RenderedStep::default()
            },
        ],
        ..RenderedJob::default()
    };
    Some(crate::seal(
        GENERATED_HEADER,
        &emit(&[
            entry("name", text("Rust cache cleanup")),
            entry(
                "on",
                Yaml::Map(vec![entry(
                    "workflow_run",
                    Yaml::Noted(
                        LINT_IGNORE.to_owned(),
                        Box::new(Yaml::Map(vec![
                            entry("workflows", Yaml::Seq(vec![text(workflow)])),
                            entry("types", Yaml::Seq(vec![text("completed")]))
                        ])),
                    )
                )])
            ),
            entry("permissions", Yaml::Map(Vec::new())),
            entry(
                "concurrency",
                Yaml::Map(vec![
                    entry(
                        "group",
                        Yaml::Expr("velnor-pr-delta-cleanup-${{ github.event.workflow_run.pull_requests[0].number || github.event.workflow_run.id }}".to_owned()),
                    ),
                    entry("cancel-in-progress", Yaml::Bool(false))
                ])
            ),
            entry("jobs", Yaml::Map(vec![entry("rust-trim", yaml_job(&job, ShellcheckMarks::Jobs))])),
        ])
    ))
}
