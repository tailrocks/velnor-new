// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_model::{
    CommitSha, Concurrency, HelperRelease, HelperVersion, Job, JobId, Permissions, Platform, PrRun,
    PullRequestType, RunnerLabel, Sha256, Step, StepKind, Triggers, WORKFLOW_PATH, When,
    WorkflowPlan,
};
use velnor_render::{
    MAX_RUN_CHARACTERS, RenderedJob, RenderedStep, RenderedWorkflow, Rule, RuleViolation,
    check_rules, render, render_workflow,
};

pub(crate) type TestResult = Result<(), Box<dyn Error>>;

const PINNED: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const SETUP_COMMIT: &str = "1793cf84b5924e2542a03b0fb2580e7395b2f4cb";
const SETUP_ACTION: &str = "tailrocks/velnor-new/actions/setup-velnor";

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

fn violations(step: RenderedStep) -> Vec<Rule> {
    match check_rules(&workflow(step)) {
        Ok(()) => Vec::new(),
        Err(found) => found.into_iter().map(|violation| violation.rule).collect(),
    }
}

fn release() -> Result<HelperRelease, Box<dyn Error>> {
    Ok(HelperRelease::new(
        HelperVersion::new("1.2.3")?,
        CommitSha::new(SETUP_COMMIT)?,
        BTreeMap::from([
            (Platform::LinuxX64, Sha256::new("a".repeat(64))?),
            (Platform::MacosArm64, Sha256::new("b".repeat(64))?),
        ]),
    )?)
}

pub(crate) fn plan(kinds: Vec<StepKind>) -> Result<WorkflowPlan, Box<dyn Error>> {
    Ok(WorkflowPlan {
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
        fork_runner: RunnerLabel::new("ubuntu-latest")?,
        hosted_runners: velnor_model::HostedRunners::new(vec![RunnerLabel::new("ubuntu-latest")?]),
        trusted_runners: velnor_model::TrustedRunners::default(),
        jobs: vec![Job {
            id: JobId::new("format")?,
            name: None,
            comment: None,
            needs: Vec::new(),
            when: When::Success,
            pr: PrRun::Always,
            runs_on: RunnerLabel::new("ubuntu-latest")?,
            permissions: Permissions::default(),
            timeout_minutes: 10,
            gate: None,
            skip_after_merge_group: None,
            plan_outputs: Vec::new(),
            group_outputs: Vec::new(),
            reports_ignored: false,
            records_build: false,
            chained_saves: false,
            selection_name: None,
            steps: kinds
                .into_iter()
                .map(|kind| Step {
                    kind,
                    directory: None,
                    manifest: None,
                    target_dir: None,
                    when: When::Success,
                    skip_after_merge_group: None,
                    skip_when_cold: false,
                    hosted_only: false,
                    pr: PrRun::Always,
                    gate: None,
                    reads: None,
                })
                .collect(),
        }],
    })
}

#[test]
fn each_rule_refuses_a_violating_step_with_its_violation() {
    let long = format!("echo {}", "a".repeat(MAX_RUN_CHARACTERS));
    let shell = RenderedStep {
        shell: Some("bash".to_owned()),
        ..run("echo hi")
    };
    let cases: Vec<(RenderedStep, Rule)> = vec![
        (run("echo one\necho two"), Rule::MultiLineRun),
        (run("if true; then echo x; fi"), Rule::ForbiddenSyntax("if")),
        (
            run("for x in a; do echo $x; done"),
            Rule::ForbiddenSyntax("for"),
        ),
        (
            run("while true; do echo x; done"),
            Rule::ForbiddenSyntax("while"),
        ),
        (
            run("case x in x) echo x;; esac"),
            Rule::ForbiddenSyntax("case"),
        ),
        (run("coproc cargo test"), Rule::ForbiddenSyntax("coproc")),
        (run("cargo test & echo done"), Rule::ForbiddenSyntax("&")),
        (run("echo a | cat"), Rule::ForbiddenSyntax("|")),
        (run("true && echo x"), Rule::ForbiddenSyntax("&&")),
        (run("false || echo x"), Rule::ForbiddenSyntax("||")),
        (run("echo a; echo b"), Rule::ForbiddenSyntax(";")),
        (run("echo $(date)"), Rule::ForbiddenSyntax("$(")),
        (run("echo `date`"), Rule::ForbiddenSyntax("`")),
        (run("cat <(echo a)"), Rule::ForbiddenSyntax("<(")),
        (run("tee >(cat)"), Rule::ForbiddenSyntax(">(")),
        (RenderedStep::default(), Rule::NotExactlyOneOfRunAndUses),
        (
            RenderedStep {
                run: Some("echo a".to_owned()),
                ..uses(PINNED)
            },
            Rule::NotExactlyOneOfRunAndUses,
        ),
        (run(&long), Rule::RunTooLong),
        (run("echo ${{ github.ref_name }}"), Rule::ExpressionInRun),
        (shell, Rule::ShellKey),
        (
            uses("evil/action@3d3c42e5aac5ba805825da76410c181273ba90b1"),
            Rule::ActionNotAllowed("evil/action".to_owned()),
        ),
        (
            uses("actions/checkout@v4"),
            Rule::ActionNotPinned("actions/checkout@v4".to_owned()),
        ),
        (
            uses("actions/checkout@3D3C42E5AAC5BA805825DA76410C181273BA90B1"),
            Rule::ActionNotPinned(
                "actions/checkout@3D3C42E5AAC5BA805825DA76410C181273BA90B1".to_owned(),
            ),
        ),
        (
            uses("actions/checkout"),
            Rule::ActionNotPinned("actions/checkout".to_owned()),
        ),
    ];
    for (step, rule) in cases {
        let found = violations(step.clone());
        assert!(
            found.contains(&rule),
            "{step:?} gave {found:?}, wanted {rule:?}"
        );
    }
}

#[test]
fn only_the_setup_action_of_this_repository_is_allowed_and_only_by_full_commit_sha() {
    let cases = [
        (
            uses(&format!("{SETUP_ACTION}@rust-ci-v0.1.81")),
            Rule::ActionNotPinned(format!("{SETUP_ACTION}@rust-ci-v0.1.81")),
        ),
        (
            uses(&format!("{SETUP_ACTION}@main")),
            Rule::ActionNotPinned(format!("{SETUP_ACTION}@main")),
        ),
        (
            uses(&format!(
                "tailrocks/velnor-new/actions/other@{SETUP_COMMIT}"
            )),
            Rule::ActionNotAllowed("tailrocks/velnor-new/actions/other".to_owned()),
        ),
    ];
    for (step, rule) in cases {
        assert!(violations(step.clone()).contains(&rule), "{step:?}");
    }
    assert_eq!(
        violations(uses(&format!("{SETUP_ACTION}@{SETUP_COMMIT}"))),
        Vec::<Rule>::new()
    );
}

#[test]
fn a_helper_release_that_could_not_work_is_refused_before_rendering() -> TestResult {
    assert!(CommitSha::new(SETUP_COMMIT).is_ok());
    for commit in [
        "",
        "main",
        "rust-ci-v1.2.3",
        &SETUP_COMMIT[..39],
        &SETUP_COMMIT.to_uppercase(),
        &format!("{SETUP_COMMIT}0"),
    ] {
        assert!(CommitSha::new(commit).is_err(), "{commit}");
    }
    for digest in ["", "ABC", &"g".repeat(64), &"a".repeat(63)] {
        assert!(Sha256::new(digest).is_err(), "{digest}");
    }
    assert!(
        HelperRelease::new(
            HelperVersion::new("1.2.3")?,
            CommitSha::new(SETUP_COMMIT)?,
            BTreeMap::new()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn emitting_a_workflow_refuses_a_violating_rendered_step_and_writes_nothing() -> TestResult {
    let plan = plan(Vec::new())?;
    let accepted = workflow(run("cargo-fmt --all --check"));
    assert!(render_workflow(&plan, &accepted).is_ok());
    let refused = render_workflow(&plan, &workflow(run("echo a; echo b")));
    assert_eq!(
        refused,
        Err(vec![RuleViolation {
            job: "format".to_owned(),
            step: 0,
            rule: Rule::ForbiddenSyntax(";"),
        }])
    );
    Ok(())
}

#[test]
fn a_violation_names_the_job_and_the_step() {
    let mut workflow = workflow(run("echo ok"));
    workflow.jobs[0].steps.push(run("echo a; echo b"));
    let found = check_rules(&workflow).err().unwrap_or_default();
    assert!(found.contains(&RuleViolation {
        job: "format".to_owned(),
        step: 1,
        rule: Rule::ForbiddenSyntax(";"),
    }));
}

#[test]
fn a_compliant_workflow_is_accepted() {
    let mut accepted = workflow(uses(PINNED));
    accepted.jobs[0].steps.extend([
        run("cargo-fmt --all --check"),
        run("velnor-ci inputs save"),
        run("velnor-gen check"),
        run("cargo build --workspace --exclude diff-tool --features diffing"),
        uses(&format!("{SETUP_ACTION}@{SETUP_COMMIT}")),
    ]);
    for action in [
        "actions/cache/restore",
        "actions/cache/save",
        "actions/upload-artifact",
        "actions/download-artifact",
        "jdx/mise-action",
    ] {
        accepted.jobs[0].steps.push(uses(&format!(
            "{action}@3d3c42e5aac5ba805825da76410c181273ba90b1"
        )));
    }
    assert_eq!(check_rules(&accepted), Ok(()));
}

#[test]
fn install_helper_renders_the_pinned_uses_step_with_the_release_tag_and_every_digest() -> TestResult
{
    let files = render(&plan(vec![StepKind::InstallHelper(release()?)])?)
        .map_err(|found| format!("{found:?}"))?;
    let text = files.get(&WORKFLOW_PATH).ok_or("no workflow")?;
    let expected = format!(
        "\
      - name: Install velnor-ci 1.2.3
        uses: {SETUP_ACTION}@{SETUP_COMMIT}  # tools-v1.2.3
        with:
          version: '1.2.3'
          sha256-linux-x64: {}
          sha256-macos-arm64: {}
",
        "a".repeat(64),
        "b".repeat(64)
    );
    assert!(text.contains(&expected), "{text}");
    Ok(())
}
