// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_model::{
    Concurrency, DownloadUrl, HelperBinary, HelperCommand, HelperRelease, HelperVersion, Job,
    JobId, Permissions, Platform, RunnerLabel, Sha256, Step, StepKind, Triggers, WorkflowPlan,
};
use velnor_render::{
    FETCH_HELPER_RUN, MAX_RUN_CHARACTERS, RenderedJob, RenderedStep, RenderedWorkflow, Rule,
    RuleViolation, Value, WORKFLOW_PATH, check_rules, render,
};

type TestResult = Result<(), Box<dyn Error>>;

const PINNED: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

fn workflow(step: RenderedStep) -> RenderedWorkflow {
    RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "format".to_owned(),
            runs_on: "ubuntu-latest".to_owned(),
            timeout_minutes: 10,
            steps: vec![step],
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
    let binary = |name: &str, digit: char| -> Result<HelperBinary, Box<dyn Error>> {
        Ok(HelperBinary {
            url: DownloadUrl::new(format!("https://example.com/rust-ci-v1.2.3/{name}"))?,
            sha256: Sha256::new(digit.to_string().repeat(64))?,
        })
    };
    Ok(HelperRelease::new(
        HelperVersion::new("1.2.3")?,
        BTreeMap::from([
            (Platform::LinuxX64, binary("velnor-ci-linux-x64", 'a')?),
            (Platform::MacosArm64, binary("velnor-ci-macos-arm64", 'b')?),
        ]),
    )?)
}

fn plan(kinds: Vec<StepKind>) -> Result<WorkflowPlan, Box<dyn Error>> {
    Ok(WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: Triggers {
            pull_request: true,
            workflow_dispatch: false,
        },
        concurrency: Concurrency {
            cancel_in_progress: true,
        },
        permissions: Permissions::default(),
        jobs: vec![Job {
            id: JobId::new("format")?,
            runs_on: RunnerLabel::new("ubuntu-latest")?,
            timeout_minutes: 10,
            steps: kinds.into_iter().map(|kind| Step { kind }).collect(),
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
        (run("cargo test & echo done"), Rule::ForbiddenSyntax("&")),
        (run("echo a | cat"), Rule::ForbiddenSyntax("|")),
        (run("true && echo x"), Rule::ForbiddenSyntax("&&")),
        (run("false || echo x"), Rule::ForbiddenSyntax("||")),
        (run("echo a; echo b"), Rule::ForbiddenSyntax(";")),
        (run("echo $(date)"), Rule::ForbiddenSyntax("$(")),
        (run("echo `date`"), Rule::ForbiddenSyntax("`")),
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
fn a_helper_release_that_could_not_work_is_refused_before_rendering() -> TestResult {
    let good = format!("https://example.com/{}", "a");
    assert!(DownloadUrl::new(good).is_ok());
    for url in [
        "https://?",
        "https://",
        "http://example.com/a",
        "https://exa mple.com",
        "https://a.com/'x",
    ] {
        assert!(DownloadUrl::new(url).is_err(), "{url}");
    }
    for digest in ["", "ABC", &"g".repeat(64), &"a".repeat(63)] {
        assert!(Sha256::new(digest).is_err(), "{digest}");
    }
    assert!(HelperRelease::new(HelperVersion::new("1.2.3")?, BTreeMap::new()).is_err());
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
        RenderedStep {
            name: "fetch".to_owned(),
            env: vec![(
                "URL".to_owned(),
                Value::Text("https://example.com".to_owned()),
            )],
            run: Some(FETCH_HELPER_RUN.to_owned()),
            ..RenderedStep::default()
        },
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
fn fetch_helper_renders_the_constant_and_a_platform_table() -> TestResult {
    let files = render(&plan(vec![StepKind::FetchHelper(release()?)])?)
        .map_err(|found| format!("{found:?}"))?;
    let text = files.get(&WORKFLOW_PATH).ok_or("no workflow")?;
    let expected = "\
    steps:
      - name: Fetch velnor-ci 1.2.3
        env:
          SHA: '${{ fromJSON(''{\"Linux-X64\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"macOS-ARM64\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"}'')[format(''{0}-{1}'', runner.os, runner.arch)] }}'
          URL: '${{ fromJSON(''{\"Linux-X64\":\"https://example.com/rust-ci-v1.2.3/velnor-ci-linux-x64\",\"macOS-ARM64\":\"https://example.com/rust-ci-v1.2.3/velnor-ci-macos-arm64\"}'')[format(''{0}-{1}'', runner.os, runner.arch)] }}'
        run: |
          curl -fsSL --create-dirs -o \"$RUNNER_TEMP/vbin/velnor-ci\" \"$URL\"
          echo \"$SHA  $RUNNER_TEMP/vbin/velnor-ci\" > \"$RUNNER_TEMP/velnor-ci.sha\"
          shasum -a 256 -c \"$RUNNER_TEMP/velnor-ci.sha\"
          chmod +x \"$RUNNER_TEMP/vbin/velnor-ci\"
          echo \"$RUNNER_TEMP/vbin\" >> \"$GITHUB_PATH\"
";
    assert!(text.ends_with(expected), "{text}");
    Ok(())
}

#[test]
fn one_command_steps_render_a_single_line_each() -> TestResult {
    let files = render(&plan(vec![
        StepKind::Helper(HelperCommand::InputsSave),
        StepKind::Helper(HelperCommand::InputsRestore),
        StepKind::GeneratorCheck,
    ])?)
    .map_err(|found| format!("{found:?}"))?;
    let text = files.get(&WORKFLOW_PATH).ok_or("no workflow")?;
    for line in [
        "run: velnor-ci inputs save",
        "run: velnor-ci inputs restore",
        "run: velnor-gen check",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    Ok(())
}

fn multi_line_runs(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(value) = line.trim_start().strip_prefix("run:") else {
            continue;
        };
        let value = value.trim();
        let indent = line.len() - line.trim_start().len();
        let quoted_open = ["'", "\""]
            .iter()
            .any(|quote| value.starts_with(quote) && (value.len() == 1 || !value.ends_with(quote)));
        if value.starts_with(['|', '>']) {
            let block: Vec<&str> = lines[index + 1..]
                .iter()
                .take_while(|next| next.starts_with(&" ".repeat(indent + 1)))
                .map(|next| next.trim_start())
                .collect();
            if block.join("\n") != FETCH_HELPER_RUN {
                found.push(block.join("\n"));
            }
        } else if value.is_empty() || quoted_open {
            found.push(value.to_owned());
        }
    }
    found
}

#[test]
fn no_golden_workflow_has_a_multi_line_run_other_than_the_fetch_constant() -> TestResult {
    let allowed = format!(
        "run: |\n{}\n",
        FETCH_HELPER_RUN
            .lines()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let rejected = [
        "run: |\n  echo one\n  echo two\n",
        "run: >\n  echo one\n",
        "run: 'echo one\n  echo two'\n",
        "run:\n  echo one\n",
    ];
    assert_eq!(multi_line_runs(&allowed), Vec::<String>::new());
    for text in rejected {
        assert_eq!(multi_line_runs(text).len(), 1, "{text}");
    }

    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    let mut checked = 0;
    for sample in fs::read_dir(samples)? {
        let workflows = sample?.path().join("expected/.github/workflows");
        for workflow in fs::read_dir(workflows)? {
            let path = workflow?.path();
            let found = multi_line_runs(&fs::read_to_string(&path)?);
            assert!(found.is_empty(), "{}: {found:?}", path.display());
            checked += 1;
        }
    }
    assert!(checked > 0);
    Ok(())
}
