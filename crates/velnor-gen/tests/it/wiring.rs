// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView, TargetKind};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const MISE_WITH_MACHETE: &str = "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = \"0.9.1\"\ncargo-deny = \"0.20.2\"\n";

fn text(view: &RepoView) -> Result<String, Box<dyn std::error::Error>> {
    Ok(workflow(view)?.replace("''", "'"))
}

fn step<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let marker = format!("name: {name}");
    let mut lines = text
        .lines()
        .skip_while(|line| !line.trim().ends_with(&marker));
    let first = lines.next().into_iter();
    first
        .chain(lines.take_while(|line| !line.trim_start().starts_with("- ")))
        .collect()
}

fn with_doctest_library(view: &mut RepoView) -> Result<(), Box<dyn std::error::Error>> {
    let lib = RepoPath::new("src/lib.rs")?;
    for package in view
        .rust_metadata
        .values_mut()
        .flat_map(|metadata| &mut metadata.members)
    {
        for target in &mut package.targets {
            target.kinds = vec![TargetKind::Lib];
            target.source = Some(lib.clone());
            target.doctest = true;
        }
    }
    Ok(())
}

fn track(view: &mut RepoView, path: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = RepoPath::new(path)?;
    view.tracked_paths.insert(path.clone());
    view.files.insert(path, text.as_bytes().to_vec());
    Ok(())
}

fn run_line<'a>(step: &[&'a str]) -> &'a str {
    step.iter()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .map(|run| run.trim_matches('\''))
        .map(|run| {
            run.split_once("\"$VELNOR_EXEC\" ")
                .map_or(run, |(_, invocation)| invocation)
        })
        .unwrap_or_default()
}

#[test]
fn the_documentation_step_builds_the_workspace_and_an_empty_selection_skips_it() -> TestResult {
    let single = text(&view(&["Cargo.toml"], None)?)?;
    let doc = step(&single, "Documentation");
    let condition = doc.join("\n");
    assert!(
        condition.contains("(steps.replan.outputs.rust_root_package_args || steps.plan.outputs.rust_root_package_args) != ''"),
        "{condition}"
    );
    assert!(
        condition.contains(
            "(steps.replan.outputs.rust_root_docs || steps.plan.outputs.rust_root_docs) == 'run'"
        ),
        "{condition}"
    );
    assert!(!condition.contains("PACKAGE_ARGS"), "{condition}");
    let run = run_line(&doc);
    assert!(run.contains("cargo doc --workspace --no-deps"), "{run}");
    assert!(!run.contains("${{"), "{run}");

    let lanes = text(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let doc = step(&lanes, "Documentation").join("\n");
    assert!(
        doc.contains("(steps.replan.outputs.rust_root_package_args || needs.rust-plan.outputs.rust_root_package_args) != ''"),
        "{doc}"
    );
    assert!(!doc.contains("PACKAGE_ARGS"), "{doc}");
    assert!(
        lanes.contains("rust_lab_package_args: '${{ steps.plan.outputs.rust_lab_package_args }}'")
    );
    Ok(())
}

#[test]
fn nextest_builds_the_workspace_and_reads_the_filter_the_plan_selected() -> TestResult {
    let single = text(&view(&["Cargo.toml"], None)?)?;
    let test = step(&single, "Test");
    let joined = test.join("\n");
    assert!(
        joined.contains("TEST_FILTER: '${{ steps.replan.outputs.rust_root_test_filter || steps.plan.outputs.rust_root_test_filter }}'"),
        "{joined}"
    );
    assert!(!joined.contains("PACKAGE_ARGS"), "{joined}");
    let run = run_line(&test);
    assert!(
        run.contains("nextest run --workspace --locked") && run.contains("-E \"$TEST_FILTER\""),
        "{run}"
    );
    assert!(!run.contains("all()") && !run.contains("${{"), "{run}");
    Ok(())
}

#[test]
fn the_doctest_step_requires_a_doctest_only_when_the_plan_found_one() -> TestResult {
    let mut files = view(
        &["lab/Cargo.toml"],
        Some(b"skip_draft_pull_requests = true\n"),
    )?;
    with_doctest_library(&mut files)?;
    let text = text(&files)?;
    let doctests = step(&text, "Doctests");
    assert_eq!(
        run_line(&doctests),
        "run --require-doctests=$DOCTESTS_KNOWN -- velnor-ci inputs build -- cargo test --doc --workspace --locked --message-format=json-render-diagnostics"
    );
    assert_eq!(text.matches("cargo test --doc").count(), 1);
    let steps: Vec<&str> = text.split("\n      - ").collect();
    let index = steps
        .iter()
        .position(|body| body.contains("name: Doctests\n"))
        .ok_or("no doctests")?;
    let build = steps.get(index - 1).ok_or("no preparatory build")?;
    assert!(
        build.contains("velnor-ci inputs build -- cargo build --workspace --all-targets --locked"),
        "{build}"
    );
    for line in steps[index].lines().filter(|line| {
        line.trim_start().starts_with("if:") || line.trim_start().starts_with("working-directory:")
    }) {
        assert!(build.lines().any(|candidate| candidate == line), "{line}");
    }
    files.files.insert(
        RepoPath::new(".velnor/rust-ci.toml")?,
        b"full_run = \"off\"\n".to_vec(),
    );
    let off = self::text(&files)?;
    assert_eq!(off.matches("cargo test --doc").count(), 1);
    Ok(())
}

#[test]
fn the_policy_lane_runs_each_tool_only_when_its_output_says_run() -> TestResult {
    let mut files = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    track(&mut files, "deny.toml", "")?;
    track(&mut files, "mise.toml", MISE_WITH_MACHETE)?;
    let text = text(&files)?;
    assert!(
        text.contains("rust_deny: '${{ steps.plan.outputs.rust_deny }}'"),
        "{text}"
    );
    assert!(
        text.contains("rust_machete: '${{ steps.plan.outputs.rust_machete }}'"),
        "{text}"
    );
    let job = text
        .lines()
        .skip_while(|line| *line != "  policy:")
        .skip(1)
        .take_while(|line| line.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(job.contains("needs:\n      - rust-plan"), "{job}");
    assert!(
        job.contains(
            "if: '${{ (github.event_name != 'pull_request' || (startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed')) && (needs.rust-plan.result == 'success' && (needs.rust-plan.outputs.rust_deny == 'run' || needs.rust-plan.outputs.rust_machete == 'run')) }}'"
        ),
        "{job}"
    );
    let deny = step(&text, "Dependency policy").join("\n");
    assert!(
        deny.contains("needs.rust-plan.outputs.rust_deny == 'run'"),
        "{deny}"
    );
    assert!(!deny.contains("rust_machete"), "{deny}");
    let machete = step(&text, "Unused dependencies").join("\n");
    assert!(
        machete.contains("needs.rust-plan.outputs.rust_machete == 'run'"),
        "{machete}"
    );
    assert!(!machete.contains("rust_deny"), "{machete}");
    Ok(())
}

#[test]
fn a_single_job_workflow_gives_the_policy_lane_its_own_plan_step() -> TestResult {
    let mut files = view(&["Cargo.toml"], None)?;
    track(&mut files, "deny.toml", "")?;
    track(&mut files, "mise.toml", MISE_WITH_MACHETE)?;
    let text = text(&files)?;
    let job = text
        .lines()
        .skip_while(|line| *line != "  policy:")
        .skip(1)
        .take_while(|line| line.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!job.contains("needs:"), "{job}");
    assert!(job.contains(r#""$VELNOR_EXEC" plan"#), "{job}");
    let deny = step(&job, "Dependency policy").join("\n");
    assert!(deny.contains("steps.plan.outcome == 'success'"), "{deny}");
    assert!(
        deny.contains("steps.plan.outputs.rust_deny == 'run'"),
        "{deny}"
    );
    Ok(())
}

fn policy_job(text: &str) -> String {
    text.lines()
        .skip_while(|line| *line != "  policy:")
        .skip(1)
        .take_while(|line| line.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_policy_lane_installs_the_pinned_tools_before_it_runs_one() -> TestResult {
    let mut files = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    track(&mut files, "deny.toml", "")?;
    track(&mut files, "mise.toml", MISE_WITH_MACHETE)?;
    let job = policy_job(&text(&files)?);
    let install = job.find("uses: jdx/mise-action").ok_or("no install step")?;
    let first = job.find("cargo-").ok_or("no policy step")?;
    assert!(install < first, "{job}");
    Ok(())
}

#[test]
fn the_policy_lane_checks_every_cargo_root() -> TestResult {
    let mut files = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    track(&mut files, "deny.toml", "")?;
    track(&mut files, "mise.toml", MISE_WITH_MACHETE)?;
    let job = policy_job(&text(&files)?);
    assert!(
        job.contains(r#""$VELNOR_EXEC" run -- cargo-deny --locked check"#),
        "{job}"
    );
    assert!(
        job.contains("cargo-deny --manifest-path 'lab/Cargo.toml' --locked check"),
        "{job}"
    );
    Ok(())
}

#[test]
fn lint_builds_the_whole_workspace_so_that_cargo_unifies_features_as_in_the_cached_build()
-> TestResult {
    let single = text(&view(&["Cargo.toml"], None)?)?;
    let lint = step(&single, "Lint");
    assert!(
        run_line(&lint).contains("clippy --workspace --all-targets"),
        "{lint:?}"
    );
    assert!(
        !lint.iter().any(|line| line.contains("PACKAGE_ARGS")),
        "{lint:?}"
    );
    Ok(())
}

#[test]
fn a_one_job_workflow_that_skips_drafts_still_lints_them() -> TestResult {
    for executable in ["Test", "Doctests"] {
        let mut repo = crate::base_save::pinned("0.1.81", "skip_draft_pull_requests = true\n")?;
        if executable == "Doctests" {
            with_doctest_library(&mut repo)?;
            for target in repo
                .rust_metadata
                .values_mut()
                .flat_map(|metadata| &mut metadata.members)
                .flat_map(|member| &mut member.targets)
            {
                target.tests = false;
            }
        }
        let text = text(&repo)?;
        let job = text
            .lines()
            .skip_while(|line| *line != "  rust:")
            .skip(1)
            .take_while(|line| line.starts_with("   "))
            .collect::<Vec<_>>()
            .join("\n");
        let draft = "github.event.pull_request.draft != true";
        assert!(
            !job.lines().take(4).any(|line| line.contains(draft)),
            "{job}"
        );
        for name in ["Lint", "Documentation", executable] {
            let command = step(&job, name);
            assert!(!command.is_empty(), "no {name}: {job}");
            let skipped = command.join("\n").contains(draft);
            assert_eq!(skipped, name == executable, "{name}: {job}");
        }
    }
    Ok(())
}

#[test]
fn a_multi_root_repository_audits_the_inputs_from_one_job_that_builds_every_root() -> TestResult {
    let config = Some(b"on_main = \"everything\"\n".as_slice());
    let single = text(&view(&["Cargo.toml"], config)?)?;
    assert_eq!(
        single.matches(r#""$VELNOR_EXEC" inputs audit"#).count(),
        1,
        "{single}"
    );
    let lanes = text(&view(&["Cargo.toml", "lab/Cargo.toml"], config)?)?;
    assert_eq!(
        lanes.matches(r#""$VELNOR_EXEC" inputs audit"#).count(),
        1,
        "{lanes}"
    );
    let audit = lanes
        .split("\n  rust-inputs-audit:\n")
        .nth(1)
        .ok_or("no audit job")?
        .split("\n  # jactionlint ignore=shellcheck\n  rust-trim:\n")
        .next()
        .ok_or("no audit job")?;
    assert!(
        audit.contains(
            "if: '${{ (github.event_name != 'pull_request' || (startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed')) && ((github.event_name == 'schedule' || github.event_name == 'workflow_dispatch' || github.event_name == 'push')) }}'"
        ),
        "{audit}"
    );
    assert_eq!(
        audit
            .matches(
                r#""$VELNOR_EXEC" run -- velnor-ci inputs build -- cargo build --workspace --all-targets --locked --message-format=json-render-diagnostics"#
            )
            .count(),
        2,
        "{audit}"
    );
    assert!(audit.contains("working-directory: lab"), "{audit}");
    assert!(audit.trim_end().ends_with("overwrite: true"), "{audit}");
    assert!(
        audit.contains(
            r#""$VELNOR_EXEC" inputs audit'
"#
        ),
        "{audit}"
    );
    Ok(())
}

#[test]
fn the_multi_root_audit_job_follows_the_schedule_policy() -> TestResult {
    let roots = ["Cargo.toml", "lab/Cargo.toml"];
    let scheduled = text(&view(&roots, Some(b"full_run = \"nightly\"\n".as_slice()))?)?;
    assert!(scheduled.contains("rust-inputs-audit:"), "{scheduled}");
    assert!(
        scheduled.contains("if: '${{ (github.event_name != 'pull_request' || (startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed')) && ((github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')) }}'"),
        "{scheduled}"
    );
    let off = text(&view(&roots, Some(b"full_run = \"off\"\n".as_slice()))?)?;
    assert!(off.contains("rust-inputs-audit:"), "{off}");
    assert!(
        off.contains(
            "((github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')) }}'"
        ),
        "{off}"
    );
    Ok(())
}
