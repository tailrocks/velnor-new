// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, check_rules};

fn workflow(step: RenderedStep) -> RenderedWorkflow {
    RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "format".to_owned(),
            runs_on: "ubuntu-latest".to_owned(),
            timeout_minutes: 10,
            steps: vec![step],
            ..RenderedJob::default()
        }],
    }
}

fn run(text: &str) -> RenderedStep {
    RenderedStep {
        name: "step".to_owned(),
        run: Some(text.to_owned()),
        ..RenderedStep::default()
    }
}

fn uses(text: &str) -> RenderedStep {
    RenderedStep {
        name: "step".to_owned(),
        uses: Some(text.to_owned()),
        ..RenderedStep::default()
    }
}

fn token_job(steps: Vec<RenderedStep>, runner: &str) -> RenderedWorkflow {
    let token = uses("actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510");
    RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "tag".to_owned(),
            runs_on: runner.to_owned(),
            timeout_minutes: 10,
            steps: steps.into_iter().chain([token]).collect(),
            ..RenderedJob::default()
        }],
    }
}

fn rules_of(workflow: &RenderedWorkflow) -> Vec<Rule> {
    check_rules(workflow)
        .err()
        .into_iter()
        .flatten()
        .map(|violation| violation.rule)
        .collect()
}

#[test]
fn a_job_with_the_release_app_token_may_not_use_mise_action_or_run_mise_with_the_repository_config()
{
    let action = "jdx/mise-action@f5be966e4a917d54244728e6a5fdcd1d93a35e6c";
    assert_eq!(
        rules_of(&token_job(vec![uses(action)], "ubuntu-latest")),
        [Rule::MiseActionInTokenJob]
    );
    assert_eq!(rules_of(&workflow(uses(action))), Vec::<Rule>::new());
    for command in [
        "mise exec aqua:release-plz/release-plz@0.4.3 -- release-plz release",
        "\"$RUNNER_TEMP/mise\" --no-config install aqua:release-plz/release-plz@0.4.3",
        "\"$RUNNER_TEMP/mise\" exec aqua:release-plz/release-plz@0.4.3 -- release-plz release",
    ] {
        assert_eq!(
            rules_of(&token_job(vec![run(command)], "ubuntu-latest")),
            [Rule::MiseReadsRepositoryConfig],
            "{command}"
        );
    }
    let isolated = "\"$RUNNER_TEMP/mise\" --no-config --no-env --no-hooks install aqua:release-plz/release-plz@0.4.3";
    assert_eq!(
        rules_of(&token_job(vec![run(isolated)], "ubuntu-latest")),
        Vec::<Rule>::new()
    );
    assert_eq!(
        rules_of(&workflow(run("mise exec rust -- cargo test"))),
        Vec::<Rule>::new()
    );
}

#[test]
fn a_job_with_the_release_app_token_runs_only_on_a_linux_x64_runner() {
    for runner in ["ubuntu-latest", "ubuntu-22.04", "ubuntu-24.04"] {
        assert_eq!(
            rules_of(&token_job(Vec::new(), runner)),
            Vec::<Rule>::new(),
            "{runner}"
        );
    }
    for runner in [
        "ubuntu-24.04-arm",
        "macos-15",
        "windows-latest",
        "self-hosted",
    ] {
        assert_eq!(
            rules_of(&token_job(Vec::new(), runner)),
            [Rule::TokenJobRunnerNotX64(runner.to_owned())],
            "{runner}"
        );
    }
}
