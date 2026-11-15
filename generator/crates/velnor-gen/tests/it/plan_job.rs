// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_rust::{CONFIG_PATH, DetectError};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

fn steps_of<'t>(text: &'t str, job: &str) -> Vec<&'t str> {
    let header = format!("  {job}:\n");
    let start = text
        .find(&header)
        .map_or(text.len(), |at| at + header.len());
    let body = &text[start..];
    let end = body
        .lines()
        .skip(1)
        .scan(0, |offset, line| {
            *offset += line.len() + 1;
            Some((*offset, line))
        })
        .find(|(_, line)| line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        .map_or(body.len(), |(offset, line)| offset - line.len() - 1);
    body[..end]
        .lines()
        .filter_map(|line| line.strip_prefix("      - name: "))
        .collect()
}

const LANE_SETUP: [&str; 7] = [
    "Check out",
    "Install velnor-ci 0.1.79",
    "Install tools",
    "Add Rust components",
    "velnor-ci prepare",
    "velnor-ci check-metadata",
    "velnor-ci restored",
];

#[test]
fn a_single_job_plans_first_and_every_later_step_waits_for_a_successful_plan_that_selects_something()
-> TestResult {
    let text = workflow(&view(&["Cargo.toml"], None)?)?;
    assert!(!text.contains("rust-plan"), "{text}");
    let steps = steps_of(&text, "rust");
    assert_eq!(
        steps[..3],
        ["Check out", "Install velnor-ci 0.1.79", "velnor-ci plan"]
    );
    let condition =
        "steps.plan.outcome == ''success'' && steps.plan.outputs.rust_root_mode != ''none''";
    let gated = text.matches(condition).count();
    let later = steps.len() - 3;
    assert_eq!(
        gated, later,
        "every step after the plan carries the gate: {text}"
    );
    assert!(
        text.contains(&format!(
            "        if: '${{{{ {condition} }}}}'\n        uses: jdx/mise-action@"
        )),
        "{text}"
    );
    assert!(text.contains(
        "        env:\n          GITHUB_TOKEN: '${{ github.token }}'\n        run: velnor-ci plan\n"
    ));
    Ok(())
}

#[test]
fn more_than_one_job_start_a_plan_job_that_installs_nothing_and_every_lane_waits_for_it()
-> TestResult {
    let text = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    assert_eq!(
        steps_of(&text, "rust-plan"),
        ["Check out", "Install velnor-ci 0.1.79", "velnor-ci plan"]
    );
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
            "    needs:\n      - rust-plan\n    if: '${{{{ needs.rust-plan.outputs.rust_{slug}_mode != ''none'' }}}}'\n"
        )));
    }
    assert!(!text.contains("_doctests"), "{text}");
    let lane = steps_of(&text, "rust-lab");
    assert_eq!(lane[..7], LANE_SETUP);
    assert!(lane[7].starts_with("Format"), "{lane:?}");
    assert!(text.contains(
        "      - rust-plan\n      - rust-root\n      - rust-lab\n    if: '${{ !cancelled() }}'\n"
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
    let doctests = "steps.plan.outputs.rust_root_doctests == ''run''";
    let docs = "steps.plan.outputs.rust_root_docs == ''run''";
    assert!(text.contains(doctests), "{text}");
    assert!(text.contains(docs), "{text}");
    for line in text
        .lines()
        .filter(|line| line.contains("cargo test --doc"))
    {
        assert!(line.contains("velnor-ci run -- cargo test --doc"), "{line}");
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
            "lab_docs",
            "root_mode",
            "root_docs"
        ]
    );
    for output in &outputs {
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
        assert!(!cargo || run.starts_with("velnor-ci run -- "), "{run}");
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

#[test]
fn the_default_branch_is_main_unless_the_config_names_another_valid_branch() -> TestResult {
    let facts = |config: &[u8]| -> Result<_, Box<dyn std::error::Error>> {
        Ok(velnor_rust::detect(&view(&["Cargo.toml"], Some(config))?))
    };
    assert_eq!(facts(b"")??.config.default_branch, "main");
    assert_eq!(
        facts(b"default_branch = \"release/2.x\"\n")??
            .config
            .default_branch,
        "release/2.x"
    );
    for bad in [
        "", "a b", "a..b", "-x", "x/", "a~b", "a:b", "a//b", "x.", "a@{b",
    ] {
        let result = facts(format!("default_branch = \"{bad}\"\n").as_bytes())?;
        assert!(
            matches!(&result, Err(DetectError::Invalid { path, .. }) if *path == CONFIG_PATH),
            "{bad:?} gave {result:?}"
        );
    }
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

#[test]
fn only_the_plan_job_may_read_pull_requests_and_every_other_job_reads_contents_only() -> TestResult
{
    let multi = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    assert_eq!(
        job_permissions(&multi, "rust-plan"),
        ["contents: read", "pull-requests: read"]
    );
    for job in ["rust-root", "rust-lab", "required"] {
        assert_eq!(job_permissions(&multi, job), ["contents: read"], "{job}");
    }
    let single = workflow(&view(&["Cargo.toml"], None)?)?;
    assert!(!single.contains("pull-requests"), "{single}");
    assert_eq!(job_permissions(&single, "rust"), ["contents: read"]);
    Ok(())
}
