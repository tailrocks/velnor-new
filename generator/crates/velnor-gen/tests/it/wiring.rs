// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView, TargetKind};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const MISE_WITH_MACHETE: &str = "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = \"0.9.1\"\n";

fn text(view: &RepoView) -> Result<String, Box<dyn std::error::Error>> {
    Ok(workflow(view)?.replace("''", "'"))
}

fn step<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let marker = format!("- name: {name}");
    let mut lines = text
        .lines()
        .skip_while(|line| !line.trim().ends_with(&marker));
    let first = lines.next().into_iter();
    first
        .chain(lines.take_while(|line| !line.trim_start().starts_with("- name:")))
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
        .unwrap_or_default()
}

#[test]
fn the_documentation_step_documents_only_the_planned_packages_and_an_empty_value_skips_it()
-> TestResult {
    let single = text(&view(&["Cargo.toml"], None)?)?;
    let doc = step(&single, "Documentation");
    let condition = doc.join("\n");
    assert!(
        condition.contains("steps.plan.outputs.rust_root_package_args != ''"),
        "{condition}"
    );
    assert!(
        condition.contains("steps.plan.outputs.rust_root_docs == 'run'"),
        "{condition}"
    );
    assert!(
        condition.contains("PACKAGE_ARGS: '${{ steps.plan.outputs.rust_root_package_args }}'"),
        "{condition}"
    );
    let run = run_line(&doc);
    assert!(run.contains("cargo doc $PACKAGE_ARGS --no-deps"), "{run}");
    assert!(
        !run.contains("--workspace") && !run.contains("${{"),
        "{run}"
    );

    let lanes = text(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let doc = step(&lanes, "Documentation").join("\n");
    assert!(
        doc.contains("needs.rust-plan.outputs.rust_root_package_args != ''"),
        "{doc}"
    );
    assert!(
        doc.contains("PACKAGE_ARGS: '${{ needs.rust-plan.outputs.rust_root_package_args }}'"),
        "{doc}"
    );
    assert!(
        lanes.contains("rust_lab_package_args: '${{ steps.plan.outputs.rust_lab_package_args }}'")
    );
    Ok(())
}

#[test]
fn nextest_reads_the_packages_and_the_filter_the_plan_selected() -> TestResult {
    let single = text(&view(&["Cargo.toml"], None)?)?;
    let test = step(&single, "Test");
    let joined = test.join("\n");
    assert!(
        joined.contains("TEST_FILTER: '${{ steps.plan.outputs.rust_root_test_filter }}'"),
        "{joined}"
    );
    assert!(
        joined.contains("PACKAGE_ARGS: '${{ steps.plan.outputs.rust_root_package_args }}'"),
        "{joined}"
    );
    let run = run_line(&test);
    assert!(
        run.contains("nextest run $PACKAGE_ARGS --locked") && run.contains("-E \"$TEST_FILTER\""),
        "{run}"
    );
    assert!(
        !run.contains("--workspace") && !run.contains("all()") && !run.contains("${{"),
        "{run}"
    );
    Ok(())
}

#[test]
fn the_doctest_step_fails_when_it_ran_no_doctest() -> TestResult {
    let mut files = view(&["Cargo.toml"], None)?;
    with_doctest_library(&mut files)?;
    let text = text(&files)?;
    let doctests = step(&text, "Doctests");
    assert_eq!(
        run_line(&doctests),
        "velnor-ci run --require-doctests -- cargo test --doc --workspace --locked"
    );
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
        .skip_while(|line| *line != "  rust-policy:")
        .skip(1)
        .take_while(|line| line.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(job.contains("needs:\n      - rust-plan"), "{job}");
    assert!(
        job.contains(
            "if: '${{ (needs.rust-plan.outputs.rust_deny == 'run' || needs.rust-plan.outputs.rust_machete == 'run') }}'"
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
    let text = text(&files)?;
    let job = text
        .lines()
        .skip_while(|line| *line != "  rust-policy:")
        .skip(1)
        .take_while(|line| line.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!job.contains("needs:"), "{job}");
    assert!(job.contains("run: velnor-ci plan"), "{job}");
    let deny = step(&job, "Dependency policy").join("\n");
    assert!(deny.contains("steps.plan.outcome == 'success'"), "{deny}");
    assert!(
        deny.contains("steps.plan.outputs.rust_deny == 'run'"),
        "{deny}"
    );
    Ok(())
}
