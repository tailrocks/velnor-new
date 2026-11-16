// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, RustMember, Shape, StepKind, Target, TargetKind};
use velnor_rust::plan;

use crate::facts::TestResult;
use crate::job_shape::{job_ids, target};
use crate::pipeline::{view, workflow};
use crate::select::{facts, member};

fn jobs_of(text: &str) -> Vec<(String, String)> {
    let mut jobs: Vec<(String, String)> = Vec::new();
    let mut in_jobs = false;
    for line in text.lines() {
        if line == "jobs:" {
            in_jobs = true;
        } else if in_jobs
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.ends_with(':')
        {
            jobs.push((line.trim().trim_end_matches(':').to_owned(), String::new()));
        } else if let Some((_, block)) = jobs.last_mut().filter(|_| in_jobs) {
            block.push_str(line);
            block.push('\n');
        }
    }
    jobs
}

fn rendered(config: &str) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    Ok(jobs_of(&workflow(&view(
        &["Cargo.toml", "lab/Cargo.toml"],
        Some(config.as_bytes()),
    )?)?))
}

#[test]
fn groups_build_one_archive_per_root_and_every_other_test_job_only_runs_it() -> TestResult {
    let jobs = rendered("shape = \"groups\"\n")?;
    let ids: Vec<&str> = jobs.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "rust-plan",
            "rust-lint-root",
            "rust-test-root",
            "rust-test-default-root",
            "rust-lint-lab",
            "rust-test-lab",
            "rust-test-default-lab",
            "rust-inputs-audit",
            "required"
        ]
    );
    let block = |id: &str| {
        jobs.iter()
            .find(|(job, _)| job == id)
            .map_or("", |(_, text)| text.as_str())
    };
    let mut names = Vec::new();
    for slug in ["root", "lab"] {
        let (build, run) = (
            block(&format!("rust-test-{slug}")),
            block(&format!("rust-test-default-{slug}")),
        );
        let artifact = format!("          name: nextest-archive-{slug}\n");
        assert_eq!(build.matches("name: Upload the tests\n").count(), 1);
        assert_eq!(run.matches("uses: actions/download-artifact@").count(), 1);
        assert!(build.contains(&artifact), "{build}");
        assert!(run.contains(&artifact), "{run}");
        names.push(artifact);
        assert!(!build.contains("nextest run"), "{build}");
        assert!(
            run.contains(&format!("      - rust-test-{slug}\n")),
            "{run}"
        );
        for forbidden in [
            "rustup",
            "velnor-ci prepare",
            "velnor-ci restored",
            "nextest archive",
            "cargo test",
            "cargo doc",
            "cargo check",
            "cargo-clippy",
            "cache/restore",
            "Compiling",
        ] {
            assert!(!run.contains(forbidden), "{slug}: {forbidden}\n{run}");
        }
        assert!(run.contains("install_args: 'aqua:nextest-rs/nextest/cargo-nextest'"));
        assert!(run.contains("--no-tests=pass"));
    }
    names.dedup();
    assert_eq!(names.len(), 2);
    Ok(())
}

#[test]
fn only_the_groups_shape_has_an_archive_and_every_nextest_step_reports_tests_run() -> TestResult {
    for (config, archives) in [
        ("", 0),
        ("shape = \"one-job\"\n", 0),
        ("shape = \"lint-test\"\n", 0),
        ("shape = \"groups\"\n", 2),
    ] {
        let jobs = rendered(config)?;
        let text: String = jobs.iter().map(|(_, block)| block.as_str()).collect();
        assert_eq!(text.matches("name: Upload the tests\n").count(), archives);
        assert_eq!(
            text.matches("uses: actions/download-artifact@").count(),
            archives
        );
        let testing: Vec<&String> = jobs
            .iter()
            .filter(|(_, block)| block.contains("nextest run"))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(testing.len(), 2, "{config}: {testing:?}");
        for (id, block) in &jobs {
            let reports =
                block.contains("      tests_run: '${{ steps.test.outputs.tests_run }}'\n");
            assert_eq!(reports, block.contains("nextest run"), "{config}: {id}");
            for line in block.lines().filter(|line| line.contains("nextest run")) {
                assert!(line.ends_with("--no-tests=pass'"), "{config}: {line}");
            }
        }
    }
    Ok(())
}

#[test]
fn a_groups_root_whose_only_tests_are_doctests_keeps_one_test_job_and_builds_no_archive() {
    let lib = RustMember {
        targets: vec![Target {
            tests: false,
            ..target(TargetKind::Lib)
        }],
        ..member("lib", "lib", &[])
    };
    let mut doctests = facts(vec![("Cargo.toml", vec![lib])]);
    doctests.config.shape = Some(Shape::BuildThenGroups);
    assert_eq!(
        job_ids(&doctests),
        ["rust-plan", "rust-lint", "rust-test", "required"]
    );
    let commands: Vec<CargoCommand> = plan(&doctests).jobs[2]
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::Cargo(command) => Some(command.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(commands, [CargoCommand::Doctest]);
}

#[test]
fn an_archive_upload_replaces_the_artifact_a_failed_attempt_left_in_the_same_run() -> TestResult {
    let jobs = rendered("shape = \"groups\"\n")?;
    let uploads: Vec<&str> = jobs
        .iter()
        .flat_map(|(_, block)| block.split("      - name: ").skip(1))
        .filter(|step| step.starts_with("Upload the tests"))
        .collect();
    assert_eq!(uploads.len(), 2, "{jobs:?}");
    for step in uploads {
        assert!(step.contains("          overwrite: true\n"), "{step}");
    }
    Ok(())
}
