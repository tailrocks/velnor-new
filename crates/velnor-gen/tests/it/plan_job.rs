// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_rust::DetectError;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

fn steps_of<'t>(text: &'t str, job: &str) -> Vec<&'t str> {
    let header = format!("  {job}:\n");
    let start = text
        .find(&header)
        .map_or(text.len(), |at| at + header.len());
    let body = &text[start..];
    body.lines()
        .take_while(|line| {
            !(line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        })
        .filter_map(|line| {
            line.strip_prefix("      - name: ")
                .or_else(|| line.strip_prefix("        name: "))
        })
        .collect()
}

const LANE_SETUP: [&str; 17] = [
    "Check out",
    "Resolve released helper tools",
    "Install velnor-ci 0.1.81",
    "Install the released helper launcher",
    "Private mbx store",
    "Restore the tools cache",
    "Install tools",
    "Add Rust components",
    "Save the tools cache",
    "velnor-ci prepare",
    "velnor-ci check-metadata",
    "Hash the sources lockfiles",
    "Restore the sources cache",
    "Restore the target cache",
    "velnor-ci inputs restore",
    "velnor-ci restored",
    "velnor-ci plan --full",
];

#[test]
fn a_single_job_plans_first_and_every_later_step_waits_for_a_successful_plan_that_selects_something()
-> TestResult {
    let text = workflow(&view(&["Cargo.toml"], None)?)?;
    assert!(!text.contains("rust-plan"), "{text}");
    let steps = steps_of(&text, "rust");
    assert_eq!(
        steps[..5],
        [
            "Check out",
            "Resolve released helper tools",
            "Install velnor-ci 0.1.81",
            "Install the released helper launcher",
            "velnor-ci plan",
        ]
    );
    let plan_index = steps
        .iter()
        .position(|name| *name == "velnor-ci plan")
        .ok_or("no plan")?;
    let condition = "steps.plan.outcome == ''success'' && (steps.replan.outputs.rust_root_mode || steps.plan.outputs.rust_root_mode) != ''none''";
    let plain =
        "steps.plan.outcome == ''success'' && steps.plan.outputs.rust_root_mode != ''none''";
    let gated = text.matches(plain).count() + text.matches(condition).count();
    let reporting = steps
        .iter()
        .filter(|name| matches!(**name, "Report cache telemetry" | "Upload cache metrics"))
        .count();
    let later = steps.len() - plan_index - 1 - reporting;
    assert_eq!(
        gated, later,
        "every step after the plan carries the gate: {text}"
    );
    assert!(
        text.contains(&format!(
            "        if: '${{{{ {plain} }}}}'\n        uses: jdx/mise-action@"
        )),
        "{text}"
    );
    assert!(text.contains(r#""$VELNOR_EXEC" plan --explain'"#));
    assert!(!text.contains("steps.velnor-helper-check-"));
    Ok(())
}

#[test]
fn more_than_one_job_start_a_plan_job_that_installs_nothing_and_every_lane_waits_for_it()
-> TestResult {
    let text = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let plan_steps = steps_of(&text, "rust-plan");
    assert_eq!(
        plan_steps[..3],
        [
            "Check out",
            "Resolve released helper tools",
            "Install velnor-ci 0.1.81"
        ]
    );
    assert_eq!(plan_steps.last(), Some(&"velnor-ci plan"));
    let plan_job = &text[text.find("  rust-plan:").unwrap_or_default()
        ..text.find("  rust-root:").unwrap_or_default()];
    for forbidden in [
        "mise-action",
        "rustup",
        "cargo",
        "prepare",
        "check-metadata",
    ] {
        assert!(!plan_job.contains(forbidden), "{forbidden} in {plan_job}");
    }
    for slug in ["root", "lab"] {
        for output in ["mode", "docs"] {
            assert!(
                text.contains(&format!(
                    "      rust_{slug}_{output}: '${{{{ steps.plan.outputs.rust_{slug}_{output} }}}}'\n"
                )),
                "{slug} {output}"
            );
        }
        assert!(text.contains(&format!(
            "    needs:\n      - rust-plan\n      - rust-inputs-audit\n    if: '${{{{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (!cancelled() && needs.rust-plan.result == ''success'' && needs.rust-plan.outputs.rust_{slug}_mode != ''none'') && (needs.rust-inputs-audit.result == ''success'' || needs.rust-inputs-audit.result == ''skipped'') }}}}'\n"
        )));
    }
    assert!(!text.contains("_doctests"), "{text}");
    let lane = steps_of(&text, "rust-lab");
    assert_eq!(lane[..LANE_SETUP.len()], LANE_SETUP);
    assert!(lane[LANE_SETUP.len()].starts_with("Format"), "{lane:?}");
    assert!(text.contains(
        "      - rust-plan\n      - rust-root\n      - rust-lab\n      - rust-inputs-audit\n    if: '${{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (always()) }}'\n"
    ));
    Ok(())
}

#[test]
fn the_doctest_and_documentation_steps_follow_the_flags_the_plan_writes() -> TestResult {
    let mut files = view(&["Cargo.toml"], None)?;
    let lib = RepoPath::new("src/lib.rs")?;
    for package in files
        .rust_metadata
        .values_mut()
        .flat_map(|m| &mut m.members)
    {
        for target in &mut package.targets {
            target.kinds = vec![velnor_model::TargetKind::Lib];
            target.source = Some(lib.clone());
            target.doctest = true;
        }
    }
    let text = workflow(&files)?;
    let doctests = "(steps.replan.outputs.rust_root_doctests || steps.plan.outputs.rust_root_doctests) == ''run''";
    let docs =
        "(steps.replan.outputs.rust_root_docs || steps.plan.outputs.rust_root_docs) == ''run''";
    assert!(text.contains(doctests), "{text}");
    assert!(text.contains(docs), "{text}");
    let known = "DOCTESTS_KNOWN: '${{ steps.replan.outputs.rust_root_doctests_known || steps.plan.outputs.rust_root_doctests_known }}'";
    assert!(text.contains(known), "{text}");
    let steps: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("cargo test --doc"))
        .collect();
    assert_ne!(steps.len(), 0);
    for line in steps {
        assert!(
            line.contains(
                "\"$VELNOR_EXEC\" run --require-doctests=$DOCTESTS_KNOWN -- velnor-ci inputs build -- cargo test --doc"
            ),
            "{line}"
        );
    }
    Ok(())
}

#[test]
fn the_plan_job_exposes_only_the_outputs_a_lane_reads() -> TestResult {
    let mut files = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    let lib = RepoPath::new("lab/src/lib.rs")?;
    let lab = files
        .rust_metadata
        .get_mut(&RepoPath::new("lab/Cargo.toml")?)
        .ok_or("lab")?;
    for target in lab.members.iter_mut().flat_map(|m| &mut m.targets) {
        target.kinds = vec![velnor_model::TargetKind::Lib];
        target.source = Some(lib.clone());
        target.doctest = true;
    }
    let text = workflow(&files)?;
    let outputs: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("      rust_"))
        .filter_map(|line| line.split(':').next())
        .collect();
    assert_eq!(
        outputs,
        [
            "lab_mode",
            "lab_doctests",
            "lab_doctests_known",
            "lab_docs",
            "lab_test_filter",
            "lab_package_args",
            "lab_tests",
            "root_mode",
            "root_docs",
            "root_test_filter",
            "root_package_args",
            "root_tests"
        ]
    );
    for output in &outputs {
        if let Some(slug) = output.strip_suffix("_tests") {
            assert!(text.contains("toJSON(needs)"));
            assert!(text.contains(&format!("--tests rust-{slug}={slug}")));
            continue;
        }
        let read = format!("needs.rust-plan.outputs.rust_{output}");
        assert!(text.contains(&read), "{output} is never read");
    }
    Ok(())
}

#[test]
fn every_cargo_command_runs_through_the_helper() -> TestResult {
    let text = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let runs: Vec<&str> = text
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("run: "))
        .map(|run| run.trim_matches('\''))
        .collect();
    assert_ne!(runs.len(), 0);
    for run in runs {
        let cargo = run.contains("cargo-") || run.starts_with("cargo ");
        assert!(!cargo || run.contains("\"$VELNOR_EXEC\" run "), "{run}");
    }
    Ok(())
}

#[test]
fn a_repository_that_does_not_pin_the_helper_cannot_generate_a_workflow() -> TestResult {
    let mut unpinned = view(&["Cargo.toml"], None)?;
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    unpinned.files.remove(&lock);
    unpinned.tracked_paths.remove(&lock);
    assert_eq!(
        expected_files(&unpinned),
        Err(GenerateError::HelperNotPinned)
    );
    Ok(())
}

#[test]
fn a_root_folder_that_would_name_a_lane_like_the_plan_job_is_an_error() -> TestResult {
    let files = view(&["Cargo.toml", "plan/Cargo.toml"], None)?;
    assert_eq!(
        expected_files(&files),
        Err(GenerateError::Detect(DetectError::JobIdTaken(
            "rust-plan".to_owned()
        )))
    );
    Ok(())
}

fn job_permissions(text: &str, job: &str) -> Vec<String> {
    let header = format!("  {job}:\n");
    let start = text
        .find(&header)
        .map_or(text.len(), |at| at + header.len());
    let body = &text[start..];
    let permissions = "    permissions:\n";
    let Some(at) = body.find(permissions) else {
        return Vec::new();
    };
    body[at + permissions.len()..]
        .lines()
        .take_while(|line| line.starts_with("      "))
        .map(|line| line.trim().to_owned())
        .collect()
}

fn checkout_depths<'t>(text: &'t str, job: &str) -> Vec<&'t str> {
    let header = format!("  {job}:\n");
    let start = text
        .find(&header)
        .map_or(text.len(), |at| at + header.len());
    text[start..]
        .lines()
        .skip_while(|line| !line.contains("name: Check out"))
        .skip(1)
        .find_map(|line| line.trim().strip_prefix("fetch-depth: "))
        .into_iter()
        .collect()
}

#[test]
fn only_the_cache_job_asks_for_pull_requests_access_and_others_read_contents() -> TestResult {
    let multi = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let single = workflow(&view(&["Cargo.toml"], None)?)?;
    for text in [&multi, &single] {
        assert_eq!(
            job_permissions(text, "rust-trim"),
            ["pull-requests: read", "actions: write"]
        );
        assert_eq!(text.matches("pull-requests").count(), 1, "{text}");
    }
    for job in ["rust-plan", "required"] {
        assert_eq!(job_permissions(&multi, job), ["contents: read"], "{job}");
    }
    for job in ["rust-root", "rust-lab"] {
        let saves = ["actions: read", "contents: read"];
        assert_eq!(job_permissions(&multi, job), saves, "{job}");
    }
    assert_eq!(
        job_permissions(&single, "rust"),
        ["actions: read", "contents: read"]
    );
    Ok(())
}

#[test]
fn a_cargo_step_runs_only_after_the_last_setup_step_succeeded() -> TestResult {
    for roots in [&["Cargo.toml"][..], &["Cargo.toml", "lab/Cargo.toml"][..]] {
        let text = workflow(&view(roots, None)?)?;
        let cargo: Vec<&str> = text
            .split("      - name: ")
            .filter(|step| step.contains(r#""$VELNOR_EXEC" run -- cargo"#))
            .collect();
        assert!(!cargo.is_empty(), "{text}");
        for step in cargo {
            assert!(
                step.contains("!cancelled() && steps.restored.outcome == ''success''"),
                "{step}"
            );
        }
        assert!(text.contains("id: restored"), "{text}");
    }
    Ok(())
}

#[test]
fn the_single_job_policy_lane_reads_the_rust_jobs_plan_and_checks_out_one_commit() -> TestResult {
    let mut files = view(&["Cargo.toml"], None)?;
    let deny = RepoPath::new("deny.toml")?;
    files.tracked_paths.insert(deny.clone());
    files.files.insert(deny, Vec::new());
    files.files.insert(
        RepoPath::new("mise.toml")?,
        b"[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n\n[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-deny = \"0.20.2\"\n".to_vec(),
    );
    let text = workflow(&files)?;
    assert!(!text.contains("rust-plan"), "{text}");
    assert!(
        !steps_of(&text, "policy").contains(&"velnor-ci plan"),
        "{text}"
    );
    assert_eq!(checkout_depths(&text, "policy"), ["1"], "{text}");
    assert_eq!(
        job_permissions(&text, "policy"),
        ["contents: read"],
        "{text}"
    );
    assert_eq!(text.matches("pull-requests").count(), 1, "{text}");
    Ok(())
}

#[test]
fn a_job_that_plans_checks_out_two_commits_and_a_job_that_does_not_checks_out_one() -> TestResult {
    let multi = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let single = workflow(&view(&["Cargo.toml"], None)?)?;
    assert_eq!(checkout_depths(&single, "rust"), ["2"]);
    assert_eq!(checkout_depths(&multi, "rust-plan"), ["2"]);
    for job in ["rust-root", "rust-lab"] {
        assert_eq!(checkout_depths(&multi, job), ["1"], "{job}");
    }
    Ok(())
}
