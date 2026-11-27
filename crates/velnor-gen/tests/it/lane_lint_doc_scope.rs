// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

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
const ARGS: &str = "(steps.replan.outputs.rust_root_package_args || needs.rust-plan.outputs.rust_root_package_args)";

fn selected(package: &str) -> String {
    let contains = |flag: &str| format!("contains(format(' {{0}} ', {ARGS}), ' {flag} ')");
    format!(
        "({} || {})",
        contains(&format!("-p {package}")),
        contains("--workspace")
    )
}

fn step(config: &str, name: &str) -> Result<(String, String), Box<dyn Error>> {
    let repo = lane_view(HELPER, config)?;
    let jobs = crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone();
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

#[test]
fn a_named_feature_lane_lints_only_its_packages_and_only_when_one_is_selected() -> TestResult {
    let (condition, run) = step(NAMED, LINT)?;
    assert!(condition.contains(&selected("package-0")), "{condition}");
    assert!(run.contains("clippy -p package-0 --all-targets"), "{run}");
    assert!(!run.contains("--workspace"), "{run}");
    Ok(())
}

#[test]
fn a_named_feature_lane_documents_only_its_packages_and_only_when_one_is_selected() -> TestResult {
    let (condition, run) = step(NAMED, DOC)?;
    assert!(condition.contains(&selected("package-0")), "{condition}");
    assert!(run.contains("cargo doc -p package-0 --no-deps"), "{run}");
    assert!(!run.contains("--workspace"), "{run}");
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
fn a_lint_and_a_doc_shared_with_a_lane_without_packages_stay_unscoped() -> TestResult {
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
fn lanes_sharing_features_and_a_package_lint_that_package_once() -> TestResult {
    let second = NAMED.replace("test(~local::)", "test(~other::)");
    let (_, run) = step(&format!("{NAMED}{second}"), LINT)?;
    assert_eq!(run.matches("-p package-0").count(), 1, "{run}");
    Ok(())
}
