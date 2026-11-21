// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_gen::GenerateError;
use velnor_model::{RepoPath, RepoView, Target, TargetKind, TargetName};
use velnor_rust::{CONFIG_PATH, DetectError, Reason};

use crate::facts::TestResult;
use crate::generated_check_source::{FIRST, FROM_SOURCE, ID, NESTED, generators, job, refused};
use crate::lint_lane::LINT;
use crate::pipeline::workflow;
use crate::private_sources::SECTION;

fn nested_repo(config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    generators(config, FIRST, &["Cargo.toml", NESTED], &[NESTED])
}

fn steps_of(job: &Value) -> Result<&[Value], Box<dyn std::error::Error>> {
    Ok(job["steps"].as_array().ok_or("no steps")?)
}

fn index_of(steps: &[Value], name: &str) -> Option<usize> {
    steps
        .iter()
        .position(|step| step["name"] == name || step["id"] == name)
}

fn target(name: &str, kind: TargetKind) -> Result<Target, Box<dyn std::error::Error>> {
    Ok(Target {
        name: TargetName::new(name)?,
        kinds: vec![kind],
        tests: false,
        doctest: false,
        source: None,
        requires_features: false,
    })
}

fn refused_with_targets(targets: &[Target]) -> Result<GenerateError, Box<dyn std::error::Error>> {
    let mut view = generators(
        &format!("{LINT}\n{FROM_SOURCE}"),
        FIRST,
        &["Cargo.toml"],
        &["Cargo.toml"],
    )?;
    let root = RepoPath::new("Cargo.toml")?;
    for member in &mut view
        .rust_metadata
        .get_mut(&root)
        .ok_or("no metadata for the root")?
        .members
    {
        member.targets = targets.to_vec();
    }
    refused(&view)
}

#[test]
fn from_source_fetches_private_git_dependencies_before_it_builds_the_generator() -> TestResult {
    let view = nested_repo(&format!("{LINT}\n{FROM_SOURCE}\n{SECTION}"))?;
    let job = job(&workflow(&view)?)?;
    let steps = steps_of(&job)?;
    let token = index_of(steps, "private-token").ok_or_else(|| format!("no token: {job}"))?;
    assert!(token < steps.len() - 1, "{job}");
    Ok(())
}

#[test]
fn from_source_runs_a_build_phase_setup_before_it_builds_the_generator() -> TestResult {
    let config = format!(
        "{LINT}\n{FROM_SOURCE}\n[[test_setup]]\nroot = 'tools/gen'\nname = 'Prepare generator fixture'\nrun = 'bash scripts/prepare-fixture.sh'\nphase = 'build'\n"
    );
    let job = job(&workflow(&nested_repo(&config)?)?)?;
    let steps = steps_of(&job)?;
    let setup =
        index_of(steps, "Prepare generator fixture").ok_or_else(|| format!("no setup: {job}"))?;
    assert!(setup < steps.len() - 1, "{job}");
    Ok(())
}

#[test]
fn from_source_reports_its_compile_to_the_cache_telemetry() -> TestResult {
    let text = workflow(&nested_repo(&format!("{LINT}\n{FROM_SOURCE}"))?)?;
    let job = job(&text)?;
    assert!(
        index_of(steps_of(&job)?, "Report cache telemetry").is_some(),
        "{job}"
    );
    let needs = crate::yaml_tree::document(&text)?["jobs"]["cache"]["needs"].clone();
    assert!(
        needs
            .as_array()
            .is_some_and(|needs| needs.iter().any(|need| need == ID)),
        "{needs}"
    );
    Ok(())
}

#[test]
fn from_source_refuses_a_generator_package_without_a_binary() -> TestResult {
    let error = refused_with_targets(&[target("velnor-gen", TargetKind::Lib)?])?;
    assert!(
        matches!(
            &error,
            GenerateError::Detect(DetectError::Invalid {
                path,
                reason: Reason::GeneratorBinary { found: 0 },
            }) if *path == CONFIG_PATH
        ),
        "{error}"
    );
    Ok(())
}

#[test]
fn from_source_refuses_a_generator_package_with_two_binaries() -> TestResult {
    let error = refused_with_targets(&[
        target("velnor-gen", TargetKind::Bin)?,
        target("other", TargetKind::Bin)?,
    ])?;
    assert!(
        matches!(
            &error,
            GenerateError::Detect(DetectError::Invalid {
                path,
                reason: Reason::GeneratorBinary { found: 2 },
            }) if *path == CONFIG_PATH
        ),
        "{error}"
    );
    Ok(())
}
