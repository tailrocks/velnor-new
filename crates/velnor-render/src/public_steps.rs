// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The steps that look the declared public git dependencies up once per workflow run.
//!
//! One step of the plan job (`velnor-ci cache public-sources`) asks the GitHub API about each
//! repository and exposes the answers as a job output. A lane's `cache decide` reads that output
//! from its own step `env`, which a step the pull request controls cannot override (a value
//! written to `$GITHUB_ENV` loses against the `env` of the step). The job token goes to these two
//! steps only, and `velnor-ci` sends it to `https://api.github.com` only.

use velnor_model::{
    PUBLIC_SOURCES_STEP, PUBLIC_VERDICTS_OUTPUT, PlanFrom, RepoPath, Step, StepKind,
};

use crate::rules::{RenderedStep, Value};

const VERDICTS_VARIABLE: &str = "VELNOR_PUBLIC_VERDICTS";
const TOKEN: &str = "${{ github.token }}";

fn manifests_env(manifests: &[RepoPath]) -> (String, Value) {
    (
        "VELNOR_CACHE_MANIFESTS".to_owned(),
        Value::Text(
            manifests
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    )
}

fn token_env() -> (String, Value) {
    ("GITHUB_TOKEN".to_owned(), Value::Expr(TOKEN.to_owned()))
}

pub(crate) fn public_sources(manifests: &[RepoPath]) -> RenderedStep {
    RenderedStep {
        id: Some(PUBLIC_SOURCES_STEP.to_owned()),
        name: "Look up the public git dependencies once for this run".to_owned(),
        continue_on_error: true,
        env: vec![manifests_env(manifests), token_env()],
        run: Some("velnor-ci cache public-sources".to_owned()),
        ..RenderedStep::default()
    }
}

/// The environment a `cache decide` step needs to use the verdicts of the run.
pub(crate) fn decide_env(from: &PlanFrom) -> Vec<(String, Value)> {
    let expression = match from {
        PlanFrom::ThisJob => format!("${{{{ steps.{PUBLIC_SOURCES_STEP}.outputs.verdicts }}}}"),
        PlanFrom::Job(job) | PlanFrom::WorkJob(job) => {
            format!("${{{{ needs.{job}.outputs.{PUBLIC_VERDICTS_OUTPUT} }}}}")
        }
    };
    vec![
        token_env(),
        (VERDICTS_VARIABLE.to_owned(), Value::Expr(expression)),
    ]
}

/// The job output that hands the verdicts to the lanes, on the job that runs the lookup.
pub(crate) fn job_output(steps: &[Step]) -> Option<(String, String)> {
    steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::PublicSources { .. }))
        .then(|| {
            (
                PUBLIC_VERDICTS_OUTPUT.to_owned(),
                format!("${{{{ steps.{PUBLIC_SOURCES_STEP}.outputs.verdicts }}}}"),
            )
        })
}

/// Renders the decide step and the lookup step, the two that read or write the verdicts.
pub(crate) fn render(kind: &StepKind) -> RenderedStep {
    match kind {
        StepKind::CacheDecide {
            target,
            archive,
            manifests,
            cli,
            budget_bytes,
            verdicts,
        } => crate::base_steps::cache_decide(
            target,
            archive.as_ref(),
            manifests,
            *cli,
            *budget_bytes,
            verdicts.as_ref(),
        ),
        StepKind::PublicSources { manifests } => public_sources(manifests),
        _ => RenderedStep::default(),
    }
}
