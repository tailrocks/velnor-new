// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::HelperRelease;
use velnor_release::ReleasePlan;

use crate::release::{ENVIRONMENT, HOSTED_RUNNER};
use crate::release_source::Source;
use crate::release_steps::{
    APP_TOKEN_OUTPUT, app_token, expression, permissions, shell_word, variables,
};
use crate::rules::{RenderedJob, RenderedStep};
use crate::steps::install_helper;

const GATE_TIMEOUT_MINUTES: u16 = 10;

fn on_the_default_branch(source: &Source) -> RenderedStep {
    RenderedStep {
        name: "Check that the tagged commit is on the default branch".to_owned(),
        env: variables(&[
            ("GH_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("COMMIT", source.verified),
            (
                "DEFAULT_BRANCH",
                "${{ github.event.repository.default_branch }}",
            ),
        ]),
        run: Some(
            "gh api \"repos/$REPOSITORY/compare/$DEFAULT_BRANCH...$COMMIT\" --jq '.status == \"behind\" or .status == \"identical\" or error(\"the tagged commit is not on the default branch\")'"
                .to_owned(),
        ),
        ..RenderedStep::default()
    }
}

fn on_the_default_branch_only(source: &Source) -> RenderedJob {
    RenderedJob {
        id: "gate".to_owned(),
        condition: Some(expression(source.passed)),
        permissions: permissions(&[("contents", "read")]),
        outputs: vec![("ready".to_owned(), "true".to_owned())],
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: GATE_TIMEOUT_MINUTES,
        steps: vec![on_the_default_branch(source)],
        ..RenderedJob::default()
    }
}

pub(crate) fn gate(plan: &ReleasePlan, helper: &HelperRelease, source: &Source) -> RenderedJob {
    if plan
        .family
        .as_ref()
        .is_some_and(|family| !family.required_checks)
    {
        return on_the_default_branch_only(source);
    }
    let mut command = "velnor-ci release-gate --repo=\"$REPOSITORY\" --sha=\"$COMMIT\"".to_owned();
    let mut scopes = vec![
        ("actions", "read"),
        ("contents", "read"),
        ("statuses", "read"),
    ];
    let mut steps = vec![install_helper(helper)];
    let mut token = "${{ github.token }}";
    if !plan.pr_only_checks.is_empty() {
        scopes.push(("checks", "read"));
        let mut app_scopes = scopes.clone();
        app_scopes.push(("administration", "write"));
        steps.push(app_token(&app_scopes));
        token = APP_TOKEN_OUTPUT;
    }
    for name in &plan.pr_only_checks {
        command.push_str(" --pr-only-check=");
        command.push_str(&shell_word(name));
    }
    if source.tag_push {
        steps.push(on_the_default_branch(source));
    }
    steps.push(RenderedStep {
        id: Some("gate".to_owned()),
        name: "Check that every required check passed on the commit".to_owned(),
        env: variables(&[
            ("GITHUB_TOKEN", token),
            ("REPOSITORY", "${{ github.repository }}"),
            ("COMMIT", source.verified),
        ]),
        run: Some(command),
        ..RenderedStep::default()
    });
    RenderedJob {
        id: "gate".to_owned(),
        condition: Some(expression(source.passed)),
        permissions: permissions(&scopes),
        environment: (!plan.pr_only_checks.is_empty()).then(|| ENVIRONMENT.to_owned()),
        outputs: vec![(
            "ready".to_owned(),
            "${{ steps.gate.outputs.ready }}".to_owned(),
        )],
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: GATE_TIMEOUT_MINUTES,
        steps,
        ..RenderedJob::default()
    }
}
