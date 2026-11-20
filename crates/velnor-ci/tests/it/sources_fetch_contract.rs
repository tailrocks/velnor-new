// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use clap::Parser;
use velnor_ci::cli::{Cli, Command as CiCommand, SourcesCommand};
use velnor_model::{
    Concurrency, Job, JobId, Permissions, PrRun, PullRequestType, RepoPath, RootSlug, RunnerLabel,
    Step, StepKind, Triggers, WORKFLOW_PATH, When, WorkflowPlan,
};
use velnor_render::render;

fn rendered_run(manifests: &[&str]) -> String {
    let manifests = manifests
        .iter()
        .map(|manifest| RepoPath::new(*manifest).unwrap_or_else(|e| unreachable!("{e}")))
        .collect();
    let plan = WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: Triggers {
            pull_request: vec![PullRequestType::Opened],
            push_branch: None,
            merge_group: false,
            schedule: None,
            workflow_dispatch: false,
        },
        concurrency: Concurrency::CancelOlderPullRequestRuns,
        permissions: Permissions::default(),
        fork_runner: RunnerLabel::new("ubuntu-latest").unwrap_or_else(|e| unreachable!("{e}")),
        hosted_runners: velnor_model::HostedRunners::new(vec![
            RunnerLabel::new("ubuntu-latest").unwrap_or_else(|e| unreachable!("{e}")),
        ]),
        trusted_runners: velnor_model::TrustedRunners::default(),
        jobs: vec![Job {
            id: JobId::new("rust").unwrap_or_else(|e| unreachable!("{e}")),
            name: None,
            comment: None,
            needs: Vec::new(),
            when: When::Success,
            pr: PrRun::Always,
            runs_on: RunnerLabel::new("ubuntu-latest").unwrap_or_else(|e| unreachable!("{e}")),
            permissions: Permissions::default(),
            timeout_minutes: 10,
            reports_ignored: false,
            records_build: false,
            selection_name: None,
            gate: None,
            skip_after_merge_group: None,
            plan_outputs: Vec::new(),
            group_outputs: Vec::new(),
            steps: vec![Step {
                kind: StepKind::SourcesFetchAll {
                    lane: RootSlug::new("root").unwrap_or_else(|e| unreachable!("{e}")),
                    manifests,
                },
                directory: None,
                manifest: None,
                target_dir: None,
                when: When::FetchSources,
                pr: PrRun::Always,
                skip_after_merge_group: None,
                skip_when_cold: false,
                hosted_only: false,
                gate: None,
                reads: None,
            }],
        }],
    };
    let files = render(&plan).unwrap_or_else(|e| unreachable!("{e:?}"));
    let text = files
        .get(&WORKFLOW_PATH)
        .unwrap_or_else(|| unreachable!("no workflow"));
    let line = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .unwrap_or_else(|| unreachable!("no run line in {text}"));
    match line
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        Some(quoted) => quoted.replace("''", "'"),
        None => line.to_owned(),
    }
}

fn argv_of(command: &str) -> Vec<String> {
    let words = command
        .strip_prefix("velnor-ci ")
        .unwrap_or_else(|| unreachable!("{command}"));
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("printf '%s\\0' {words}"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|word| !word.is_empty())
        .map(|word| String::from_utf8_lossy(word).into_owned())
        .collect()
}

fn parsed_roots(manifests: &[&str]) -> Vec<PathBuf> {
    let run = rendered_run(manifests);
    let words = argv_of(&run);
    let cli = Cli::try_parse_from(std::iter::once("velnor-ci".to_owned()).chain(words))
        .unwrap_or_else(|e| unreachable!("the helper rejects {run}: {e}"));
    match cli.command {
        CiCommand::Sources(SourcesCommand::Fetch(args)) => args.roots,
        other => unreachable!("{run} parsed as {other:?}"),
    }
}

#[test]
fn the_generated_fetch_step_is_a_command_the_helper_parses_with_every_root_in_order() {
    assert_eq!(
        parsed_roots(&["Cargo.toml", "xtask/Cargo.toml", "a/b/Cargo.toml"]),
        [".", "./xtask", "./a/b"].map(PathBuf::from)
    );
}

#[test]
fn roots_with_spaces_quotes_or_a_leading_dash_stay_one_argument_each() {
    assert_eq!(
        parsed_roots(&[
            "-odd/Cargo.toml",
            "my root/Cargo.toml",
            "it's/Cargo.toml",
            "$HOME/x/Cargo.toml",
        ]),
        ["./-odd", "./my root", "./it's", "./$HOME/x"].map(PathBuf::from)
    );
}
