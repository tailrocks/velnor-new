// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    GateNeed, PUBLIC_SOURCES_STEP, PUBLIC_VERDICTS_OUTPUT, PlanFrom, RepoPath, RootSlug, Step,
    StepKind,
};

use crate::gate::{PLAN_STEP, output};
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

fn selected_condition(selected: &[RootSlug]) -> Option<String> {
    (!selected.is_empty()).then(|| {
        let reads: Vec<String> = selected
            .iter()
            .map(|slug| {
                format!(
                    "steps.{PLAN_STEP}.outputs.{} != 'none'",
                    output(slug, GateNeed::Selected)
                )
            })
            .collect();
        format!(
            "${{{{ steps.{PLAN_STEP}.outcome == 'success' && ({}) }}}}",
            reads.join(" || ")
        )
    })
}

pub(crate) fn public_sources(manifests: &[RepoPath], selected: &[RootSlug]) -> RenderedStep {
    RenderedStep {
        condition: selected_condition(selected),
        id: Some(PUBLIC_SOURCES_STEP.to_owned()),
        name: "Look up the public git dependencies once for this run".to_owned(),
        continue_on_error: true,
        env: vec![manifests_env(manifests), token_env()],
        run: Some("velnor-ci cache public-sources".to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn decide_env(from: &PlanFrom) -> Vec<(String, Value)> {
    let expression = match from {
        PlanFrom::ThisJob => format!("${{{{ steps.{PUBLIC_SOURCES_STEP}.outputs.verdicts }}}}"),
        PlanFrom::Job(job) | PlanFrom::WorkJob(job) => {
            format!("${{{{ needs.{job}.outputs.{PUBLIC_VERDICTS_OUTPUT} }}}}")
        }
    };
    vec![(VERDICTS_VARIABLE.to_owned(), Value::Expr(expression))]
}

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
        StepKind::PublicSources {
            manifests,
            selected,
        } => public_sources(manifests, selected),
        _ => RenderedStep::default(),
    }
}
