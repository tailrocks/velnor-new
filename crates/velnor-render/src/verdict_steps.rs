// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::{Value as Json, json};
use velnor_model::{
    Job, REUSE_OUTPUT, REUSE_RUN_OUTPUT, REUSE_STEP, REUSED_OUTPUT, Reuse, ReuseStep, Step,
    StepKind,
};

use crate::gate::{REPLAN_STEP, all_conditions};
use crate::rules::{RenderedJob, RenderedReuse, RenderedStep, Value};
use crate::steps::UPLOAD_ARTIFACT;

const ARTIFACT: &str = "velnor-verdict";
const RETENTION_DAYS: u64 = 7;
const REUSABLE_VARIABLE: &str = "VELNOR_REUSABLE";
const SAME_REPOSITORY_PULL_REQUEST: &str = "github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository";

fn plan_json() -> (String, Value) {
    (
        "PLAN_JSON".to_owned(),
        Value::Expr("${{ toJSON(steps.plan.outputs) }}".to_owned()),
    )
}

pub(crate) fn reuse_step(step: &ReuseStep) -> RenderedStep {
    match step {
        ReuseStep::Record => record_step(),
        ReuseStep::Upload => upload_step(),
        ReuseStep::Decide { required } => decide_step(required),
    }
}

fn record_step() -> RenderedStep {
    RenderedStep {
        name: "velnor-ci verdict record".to_owned(),
        id: Some("verdict_record".to_owned()),
        condition: Some(format!("${{{{ {SAME_REPOSITORY_PULL_REQUEST} }}}}")),
        continue_on_error: true,
        env: std::iter::once(plan_json()).collect(),
        run: Some("velnor-ci verdict record".to_owned()),
        ..RenderedStep::default()
    }
}

fn upload_step() -> RenderedStep {
    RenderedStep {
        name: "Upload the verdict".to_owned(),
        condition: Some(format!(
            "${{{{ {SAME_REPOSITORY_PULL_REQUEST} && steps.verdict_record.outcome == 'success' }}}}"
        )),
        continue_on_error: true,
        uses: Some(UPLOAD_ARTIFACT.to_owned()),
        with: vec![
            ("name".to_owned(), Value::Text(ARTIFACT.to_owned())),
            (
                "path".to_owned(),
                Value::Expr("${{ runner.temp }}/velnor-verdict".to_owned()),
            ),
            (
                "if-no-files-found".to_owned(),
                Value::Text("error".to_owned()),
            ),
            ("retention-days".to_owned(), Value::Int(RETENTION_DAYS)),
            ("overwrite".to_owned(), Value::Bool(true)),
        ],
        ..RenderedStep::default()
    }
}

fn decide_step(required: &str) -> RenderedStep {
    let expression = |name: &str, value: &str| (name.to_owned(), Value::Expr(value.to_owned()));
    RenderedStep {
        name: "velnor-ci verdict reuse".to_owned(),
        id: Some(REUSE_STEP.to_owned()),
        condition: Some("${{ github.event_name == 'merge_group' }}".to_owned()),
        continue_on_error: true,
        env: [
            expression("GITHUB_TOKEN", "${{ github.token }}"),
            expression("VELNOR_API_URL", "${{ github.api_url }}"),
            expression(
                "VELNOR_MERGE_GROUP_HEAD_REF",
                "${{ github.event.merge_group.head_ref }}",
            ),
            expression(
                "VELNOR_MERGE_GROUP_BASE_SHA",
                "${{ github.event.merge_group.base_sha }}",
            ),
            (
                "VELNOR_REQUIRED_CHECK".to_owned(),
                Value::Text(required.to_owned()),
            ),
            (REUSABLE_VARIABLE.to_owned(), Value::Text("[]".to_owned())),
            plan_json(),
        ]
        .into_iter()
        .collect(),
        run: Some("velnor-ci verdict reuse".to_owned()),
        ..RenderedStep::default()
    }
}

fn contains(plan: &str, id: &str) -> String {
    format!("contains(format(' {{0}} ', needs.{plan}.outputs.{REUSED_OUTPUT}), ' {id} ')")
}

pub(crate) fn reused_term(reuse: &Reuse) -> String {
    contains(reuse.plan.as_str(), &reuse.id)
}

pub(crate) fn skipped(reuse: &Reuse, replanned: bool) -> String {
    skip_term(reuse.plan.as_str(), &reuse.id, replanned)
}

fn skip_term(plan: &str, id: &str, replanned: bool) -> String {
    let term = contains(plan, id);
    if replanned {
        format!("(!{term} || steps.{REPLAN_STEP}.outcome == 'success')")
    } else {
        format!("!{term}")
    }
}

fn build_term(plan: &str, id: &str, replanned: bool) -> String {
    let term = contains(plan, id);
    if replanned {
        format!("({term} && steps.{REPLAN_STEP}.outcome != 'success')")
    } else {
        term
    }
}

pub(crate) fn outputs(job: &Job) -> impl Iterator<Item = (String, String)> {
    job.steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::Reuse(ReuseStep::Decide { .. })))
        .then(|| {
            [REUSE_OUTPUT, REUSE_RUN_OUTPUT, REUSED_OUTPUT].map(|name| {
                (
                    name.to_owned(),
                    format!("${{{{ steps.{REUSE_STEP}.outputs.{name} }}}}"),
                )
            })
        })
        .into_iter()
        .flatten()
}

fn listed(jobs: &[Job], rendered: &[RenderedJob]) -> Vec<Json> {
    let mut items = Vec::new();
    for (job, shown) in jobs.iter().zip(rendered) {
        if let Some(reuse) = &job.reuse {
            let name = shown.name.clone().unwrap_or_else(|| shown.id.clone());
            items.push(json!({"id": reuse.id, "job": name}));
        }
        let name = shown.name.clone().unwrap_or_else(|| shown.id.clone());
        items.extend(shown.steps.iter().filter_map(|step| {
            let reuse = step.reuse.as_ref().filter(|reuse| !reuse.build_only)?;
            Some(json!({"id": reuse.id, "job": name, "step": step.name}))
        }));
    }
    items
}

pub(crate) fn fill_reusable(jobs: &[Job], rendered: &mut [RenderedJob]) {
    let list = Json::Array(listed(jobs, rendered)).to_string();
    for step in rendered
        .iter_mut()
        .flat_map(|job| &mut job.steps)
        .filter(|step| step.id.as_deref() == Some(REUSE_STEP))
    {
        for (name, value) in &mut step.env {
            if name == REUSABLE_VARIABLE {
                *value = Value::Text(list.clone());
            }
        }
    }
}

pub(crate) fn job_condition(condition: String, job: &Job) -> String {
    match &job.reuse {
        Some(reuse) => all_conditions(
            Some(condition),
            Some(format!("${{{{ {} }}}}", skipped(reuse, false))),
        )
        .unwrap_or_default(),
        None => condition,
    }
}

pub(crate) fn with_twin(step: &Step, mut body: RenderedStep) -> Vec<RenderedStep> {
    let Some(reuse) = &step.reuse else {
        return vec![body];
    };
    let root = step
        .manifest
        .as_ref()
        .map_or(".", |manifest| {
            manifest
                .as_str()
                .rsplit_once('/')
                .map_or(".", |(directory, _)| directory)
        })
        .to_owned();
    body.reuse = Some(RenderedReuse {
        id: reuse.id.clone(),
        plan: reuse.plan.to_string(),
        build_only: false,
        root: root.clone(),
    });
    if !matches!(&step.kind, StepKind::Cargo(command) if command.reports_tests()) {
        return vec![body];
    }
    let mut twin = body.clone();
    twin.name = format!("{} (build only)", body.name);
    twin.id = None;
    twin.reuse = Some(RenderedReuse {
        id: reuse.id.clone(),
        plan: reuse.plan.to_string(),
        build_only: true,
        root,
    });
    twin.run = body.run.as_deref().and_then(build_only);
    vec![twin, body]
}

const BUILD_ONLY_SUFFIX: &str = " (build only)";

pub(crate) fn qualify_shared(steps: &mut [RenderedStep]) {
    let base = |step: &RenderedStep| {
        let name = step.name.as_str();
        name.strip_suffix(BUILD_ONLY_SUFFIX)
            .unwrap_or(name)
            .to_owned()
    };
    let reusable =
        |step: &&RenderedStep| step.reuse.as_ref().is_some_and(|reuse| !reuse.build_only);
    let shared: BTreeSet<String> = steps
        .iter()
        .filter(reusable)
        .filter(|step| {
            steps
                .iter()
                .filter(reusable)
                .filter(|other| other.name == step.name)
                .count()
                > 1
        })
        .map(|step| step.name.clone())
        .collect();
    for step in steps.iter_mut() {
        let Some(reuse) = &step.reuse else { continue };
        let name = base(step);
        if shared.contains(&name) {
            let suffix = if reuse.build_only {
                BUILD_ONLY_SUFFIX
            } else {
                ""
            };
            step.name = format!("{name} ({}){suffix}", reuse.root);
        }
    }
}

fn shell_words(line: &str) -> Option<Vec<&str>> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    for (at, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        let next = line[at + c.len_utf8()..].chars().next();
        match (quote, c) {
            (Some('\''), '\'') => quote = None,
            (Some('\''), _) => {}
            (_, '`') => return None,
            (_, '$') if matches!(next, Some('(' | '\'')) => return None,
            (None | Some('"'), '\\') => escaped = true,
            (Some(_), '"') => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, ' ') => {
                if start < at {
                    words.push(&line[start..at]);
                }
                start = at + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || escaped {
        return None;
    }
    if start < line.len() {
        words.push(&line[start..]);
    }
    Some(words)
}

fn build_only(run: &str) -> Option<String> {
    let words = shell_words(run)?;
    let at = words
        .iter()
        .position(|word| word.starts_with("cargo"))
        .unwrap_or(0);
    let (wrapper, command) = words.split_at(at);
    Some(
        wrapper
            .iter()
            .map(|word| (*word).to_owned())
            .chain(velnor_model::no_run(command))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

pub(crate) fn for_body(extra: Vec<String>, body: &RenderedStep, replanned: bool) -> Vec<String> {
    let Some(reuse) = body.reuse.as_ref().filter(|reuse| reuse.build_only) else {
        return extra;
    };
    let skipped = skip_term(&reuse.plan, &reuse.id, replanned);
    extra
        .into_iter()
        .map(|part| {
            if part == skipped {
                build_term(&reuse.plan, &reuse.id, replanned)
            } else {
                part
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
