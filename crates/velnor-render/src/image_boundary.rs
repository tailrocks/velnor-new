// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedJob, RenderedStep, Rule, RuleViolation, Value};

const SECRET_NAMES: &[&str] = &["TOKEN", "PASSWORD", "SECRET", "CREDENTIAL", "AUTH", "KEY"];
const SECRET_EXPRESSIONS: &[&str] = &["secrets.", "github.token", "vars.", "app-token"];
const CODE_JOB_SCOPES: &[(&str, &str)] = &[("contents", "read"), ("pull-requests", "read")];

fn runs_repository_code(step: &RenderedStep) -> bool {
    step.run
        .as_deref()
        .is_some_and(|run| run.contains("cargo") || run.contains("$VELNOR_EXEC\" run "))
        || step
            .uses
            .as_deref()
            .is_some_and(|uses| uses.starts_with("./"))
}

fn text(value: &Value) -> &str {
    match value {
        Value::Text(text) | Value::Expr(text) | Value::BoundedExpr { text, .. } => text,
        Value::Int(_) | Value::Bool(_) => "",
    }
}

fn carries_a_secret(step: &RenderedStep) -> bool {
    let named = step.env.iter().any(|(name, value)| {
        let upper = name.to_ascii_uppercase();
        SECRET_NAMES.iter().any(|marker| upper.contains(marker))
            || SECRET_EXPRESSIONS
                .iter()
                .any(|marker| text(value).contains(marker))
    });
    named
        || step.with.iter().any(|(name, value)| {
            let upper = name.to_ascii_uppercase();
            SECRET_NAMES.iter().any(|marker| upper.contains(marker))
                || SECRET_EXPRESSIONS
                    .iter()
                    .any(|marker| text(value).contains(marker))
        })
        || step.uses.as_deref().is_some_and(|uses| {
            uses.contains("login-action") || uses.contains("create-github-app-token")
        })
}

fn violation(job: &RenderedJob, step: usize, why: &'static str) -> RuleViolation {
    RuleViolation {
        job: job.id.clone(),
        step,
        rule: Rule::ImageBoundary(why),
    }
}

#[must_use]
pub fn check_image_boundary(jobs: &[RenderedJob]) -> Vec<RuleViolation> {
    let mut found = Vec::new();
    for job in jobs {
        let first_code = job.steps.iter().position(runs_repository_code);
        if job.environment.is_some()
            && let Some(at) = first_code
        {
            found.push(violation(
                job,
                at,
                "a job in an environment, which holds credentials, runs repository code",
            ));
        }
        let Some(first) = first_code else {
            if job.environment.is_none()
                && let Some(at) = job.steps.iter().position(|step| {
                    step.uses
                        .as_deref()
                        .is_some_and(|uses| uses.contains("login-action"))
                })
            {
                found.push(violation(
                    job,
                    at,
                    "a registry login sits in a job that has no environment",
                ));
            }
            continue;
        };
        if job
            .permissions
            .iter()
            .any(|scope| !CODE_JOB_SCOPES.contains(&(scope.0.as_str(), scope.1.as_str())))
        {
            found.push(violation(
                job,
                first,
                "a job that runs repository code holds a permission beyond contents: read and pull-requests: read",
            ));
        }
        if !job.permissions.iter().any(|(name, _)| name == "contents") {
            found.push(violation(
                job,
                first,
                "a job that runs repository code does not state contents: read",
            ));
        }
        if job.env.iter().any(|(name, value)| {
            carries_a_secret(&RenderedStep {
                env: vec![(name.clone(), value.clone())],
                ..RenderedStep::default()
            })
        }) {
            found.push(violation(
                job,
                first,
                "a job that runs repository code has a secret in its job environment",
            ));
        }
        for (index, step) in job.steps.iter().enumerate().skip(first) {
            if carries_a_secret(step) {
                found.push(violation(
                    job,
                    index,
                    "a credential reaches a step at or after the first step that runs repository code",
                ));
            }
        }
    }
    found
}
