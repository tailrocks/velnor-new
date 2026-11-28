// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{PackageName, RepoPath, RepoView, RustPackage};

use crate::facts::TestResult;
use crate::feature_lanes::{lane_view, steps_named};
use crate::pipeline::workflow;

const HELPER: &str = "0.1.108";
const NAMED: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\nlint = true\ndoc = true\n";
const ALL: &str = "[[test_lane]]\nroot = '.'\nall_features = true\npackages = ['package-0']\nfilter = 'test(~all::)'\nlint = true\ndoc = true\n";
const NO_DEFAULT: &str = "[[test_lane]]\nroot = '.'\nno_default_features = true\npackages = ['package-0']\nfilter = 'test(~nodefault::)'\nlint = true\ndoc = true\n";
const WITHOUT_PACKAGES: &str =
    "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\nlint = true\ndoc = true\n";
const LINT: &str = "Lint (features: local-mode)";
const DOC: &str = "Documentation (features: local-mode)";
const ARGS: &str = "env.VELNOR_PACKAGE_ARGS_ROOT";

fn selected(package: &str) -> String {
    let contains = |flag: &str| format!("contains({ARGS}, ' {flag} ')");
    format!(
        "(steps.replan.outcome == 'success' || {} || {})",
        contains(&format!("-p {package}")),
        contains("--workspace")
    )
}

fn step(config: &str, name: &str) -> Result<(String, String), Box<dyn Error>> {
    step_in(&lane_view(HELPER, config)?, name)
}

fn with_member(repo: &mut RepoView, name: &str, manifest_text: &str) -> Result<(), Box<dyn Error>> {
    let manifest = RepoPath::new(format!("{name}/Cargo.toml"))?;
    repo.tracked_paths.insert(manifest.clone());
    repo.files
        .insert(manifest.clone(), manifest_text.as_bytes().to_vec());
    let members = &mut repo
        .rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no root")?
        .members;
    let template = members[0].clone();
    members.push(RustPackage {
        name: PackageName::new(name)?,
        manifest,
        ..template
    });
    Ok(())
}

fn step_in(repo: &RepoView, name: &str) -> Result<(String, String), Box<dyn Error>> {
    let jobs = crate::yaml_tree::document(&workflow(repo)?)?["jobs"].clone();
    let found = steps_named(&jobs, name);
    assert_eq!(found.len(), 1, "{name}: {found:?}");
    let text = |key: &str| -> Result<String, Box<dyn Error>> {
        Ok(found[0].2[key]
            .as_str()
            .ok_or_else(|| format!("no {key}"))?
            .to_owned())
    };
    Ok((text("if")?, text("run")?))
}

fn lane_with(features: &str) -> String {
    NAMED.replace("local-mode", features)
}

fn gated_on(condition: &str, package: &str) -> bool {
    condition.contains(&format!("' -p {package} '"))
}

#[test]
fn a_named_feature_lane_lints_the_workspace_and_gates_on_its_packages() -> TestResult {
    let (condition, run) = step(NAMED, LINT)?;
    assert!(condition.contains(&selected("package-0")), "{condition}");
    assert!(run.contains("clippy --workspace --all-targets"), "{run}");
    assert!(!run.contains("-p package-0"), "{run}");
    Ok(())
}

#[test]
fn a_named_feature_lane_documents_the_workspace_and_gates_on_its_packages() -> TestResult {
    let (condition, run) = step(NAMED, DOC)?;
    assert!(condition.contains(&selected("package-0")), "{condition}");
    assert!(run.contains("cargo doc --workspace --no-deps"), "{run}");
    assert!(!run.contains("-p package-0"), "{run}");
    Ok(())
}

#[test]
fn an_all_features_or_no_default_features_lane_lints_and_documents_the_workspace() -> TestResult {
    for (config, label) in [(ALL, "all features"), (NO_DEFAULT, "no default features")] {
        let (lint, lint_run) = step(config, &format!("Lint ({label})"))?;
        assert!(!lint.contains("package_args"), "{label}: {lint}");
        assert!(
            lint_run.contains("clippy --workspace"),
            "{label}: {lint_run}"
        );
        let (doc, doc_run) = step(config, &format!("Documentation ({label})"))?;
        assert!(!doc.contains(&selected("package-0")), "{label}: {doc}");
        assert!(
            doc_run.contains("cargo doc --workspace"),
            "{label}: {doc_run}"
        );
    }
    Ok(())
}

#[test]
fn a_lint_and_a_doc_shared_with_a_lane_without_packages_are_not_gated_on_packages() -> TestResult {
    let config = format!("{NAMED}{WITHOUT_PACKAGES}");
    let (lint, lint_run) = step(&config, LINT)?;
    assert!(!lint.contains("package_args"), "{lint}");
    assert!(lint_run.contains("clippy --workspace"), "{lint_run}");
    let (doc, doc_run) = step(&config, DOC)?;
    assert!(!doc.contains(&selected("package-0")), "{doc}");
    assert!(doc_run.contains("cargo doc --workspace"), "{doc_run}");
    Ok(())
}

#[test]
fn lanes_sharing_features_and_a_package_gate_on_that_package_once() -> TestResult {
    let second = NAMED.replace("test(~local::)", "test(~other::)");
    let (condition, _) = step(&format!("{NAMED}{second}"), LINT)?;
    assert_eq!(
        condition.matches("' -p package-0 '").count(),
        1,
        "{condition}"
    );
    Ok(())
}

#[test]
fn a_member_defining_the_lane_feature_joins_the_gate_of_the_lane_packages() -> TestResult {
    let mut repo = lane_view(HELPER, NAMED)?;
    with_member(&mut repo, "definer", "[features]\nlocal-mode = []\n")?;
    with_member(&mut repo, "bystander", "[features]\nother = []\n")?;
    for (name, command) in [(LINT, "clippy"), (DOC, "cargo doc")] {
        let (condition, run) = step_in(&repo, name)?;
        assert!(run.contains(&format!("{command} --workspace ")), "{run}");
        assert!(!run.contains("-p definer"), "{run}");
        assert!(gated_on(&condition, "definer"), "{condition}");
        assert!(gated_on(&condition, "package-0"), "{condition}");
        assert!(!condition.contains("bystander"), "{condition}");
    }
    Ok(())
}

#[test]
fn a_member_with_an_optional_dependency_of_the_lane_feature_name_joins_the_gate() -> TestResult {
    let manifests = [
        "[dependencies]\nx = { version = \"1\", optional = true }\n",
        "[dependencies]\nx = { package = \"other\", version = \"1\", optional = true }\n",
    ];
    for manifest in manifests {
        let mut repo = lane_view(HELPER, &lane_with("x"))?;
        with_member(&mut repo, "optional", manifest)?;
        for name in ["Lint (features: x)", "Documentation (features: x)"] {
            let (condition, _) = step_in(&repo, name)?;
            assert!(gated_on(&condition, "optional"), "{manifest}: {condition}");
        }
    }
    Ok(())
}

#[test]
fn lanes_sharing_features_with_different_packages_gate_on_the_union() -> TestResult {
    let mut repo = lane_view(
        HELPER,
        &format!(
            "{NAMED}{}",
            NAMED
                .replace("package-0", "package-1")
                .replace("local::", "two::")
        ),
    )?;
    with_member(&mut repo, "package-1", "")?;
    for name in [LINT, DOC] {
        let (condition, run) = step_in(&repo, name)?;
        assert!(gated_on(&condition, "package-0"), "{condition}");
        assert!(gated_on(&condition, "package-1"), "{condition}");
        assert!(run.contains("--workspace"), "{run}");
    }
    Ok(())
}

#[test]
fn a_lane_whose_feature_is_defined_by_60_long_named_members_keeps_its_gate_under_the_limit()
-> TestResult {
    let definer = |index: usize| format!("definer-{}{index}", "x".repeat(230));
    let mut repo = lane_view(HELPER, NAMED)?;
    for index in 0..60 {
        with_member(&mut repo, &definer(index), "[features]\nlocal-mode = []\n")?;
    }
    for name in [LINT, DOC] {
        let (condition, _) = step_in(&repo, name)?;
        assert!(gated_on(&condition, &definer(59)), "{name}");
        assert!(
            condition.chars().count() <= 21_000,
            "{name}: {}",
            condition.len()
        );
    }
    Ok(())
}
