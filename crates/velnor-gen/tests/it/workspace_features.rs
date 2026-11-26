// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{RepoPath, RepoView, Target, TargetKind, TargetName};
use velnor_render::rendered_workflow;
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect, plan};

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};
use crate::test_groups::{DATABASE, SHELL};
use crate::yaml_tree::document;

const FEATURES: &str = "workspace_features = ['local-mode']\n";
const MANIFEST: &[u8] = b"[package]\nname = \"package-0\"\n[features]\nlocal-mode = []\n";

fn repository(roots: &[&str], config: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = view(roots, Some(config.as_bytes()))?;
    for root in roots {
        repo.files.insert(RepoPath::new(*root)?, MANIFEST.to_vec());
    }
    for metadata in repo.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            member.targets.push(Target {
                name: TargetName::new("lib")?,
                kinds: vec![TargetKind::Lib],
                tests: false,
                doctest: true,
                source: None,
                requires_features: false,
                required_features: Vec::new(),
            });
        }
    }
    Ok(repo)
}

fn runs(config: &str) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let text = workflow(&repository(&["Cargo.toml"], config)?)?;
    let jobs = document(&text)?["jobs"].clone();
    Ok(jobs
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, job)| job["steps"].as_array().cloned().unwrap_or_default())
        .filter_map(|step| {
            Some((
                step["name"].as_str()?.to_owned(),
                step["run"].as_str()?.to_owned(),
            ))
        })
        .collect())
}

fn planned(
    roots: &[&str],
    config: &str,
    pin: &str,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut repo = repository(roots, config)?;
    pin_helper(&mut repo, pin)?;
    let rendered = rendered_workflow(&plan(&detect(&repo)?));
    Ok(rendered
        .jobs
        .iter()
        .flat_map(|job| &job.steps)
        .filter_map(|step| Some((step.name.clone(), step.run.clone()?)))
        .collect())
}

fn named<'a>(steps: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    steps
        .iter()
        .filter(|(step, _)| step == name)
        .map(|(_, run)| run.as_str())
        .collect()
}

fn assert_featured(steps: &[(String, String)], name: &str, featured: bool) {
    let found = named(steps, name);
    assert!(!found.is_empty(), "no step {name} in {steps:?}");
    for run in found {
        assert_eq!(
            run.contains(" --features 'local-mode'"),
            featured,
            "{name}: {run}"
        );
    }
}

#[test]
fn workspace_features_apply_to_the_main_lint_doc_and_test_and_no_second_lane_exists() -> TestResult
{
    let steps = runs(FEATURES)?;
    for name in ["Lint", "Documentation", "Test", "Doctests"] {
        assert_eq!(named(&steps, name).len(), 1, "{name}: {steps:?}");
        assert_featured(&steps, name, true);
    }
    let lanes: Vec<_> = steps
        .iter()
        .filter(|(name, _)| name.contains("(features:"))
        .collect();
    assert!(lanes.is_empty(), "{lanes:?}");
    let plain = runs("")?;
    for name in ["Lint", "Documentation", "Test", "Doctests"] {
        assert_featured(&plain, name, false);
    }
    Ok(())
}

#[test]
fn the_archive_the_groups_run_from_is_built_with_the_features_and_the_overlap_check_lists_that_build()
-> TestResult {
    let steps = runs(&format!("{FEATURES}{DATABASE}\n{SHELL}"))?;
    let archive = named(&steps, "Build the tests");
    assert_eq!(archive.len(), 1, "{steps:?}");
    assert!(archive[0].contains("nextest archive"), "{archive:?}");
    assert_featured(&steps, "Build the tests", true);
    assert_featured(&steps, "Lint", true);
    assert_featured(&steps, "Documentation", true);
    assert_featured(&steps, "Doctests", true);
    assert_featured(&steps, "Check package-0 alone", false);
    assert_featured(&steps, "Test", false);
    let check = named(&steps, "velnor-ci test-groups check");
    assert_eq!(check.len(), 1, "{steps:?}");
    assert!(
        check[0].contains("test-groups check --features 'local-mode' --group 'database="),
        "{check:?}"
    );
    let plain = runs(&format!("{DATABASE}\n{SHELL}"))?;
    assert_featured(&plain, "Build the tests", false);
    assert!(
        !named(&plain, "velnor-ci test-groups check")[0].contains("--features"),
        "{plain:?}"
    );
    Ok(())
}

#[test]
fn the_audit_build_of_every_target_and_the_doctests_take_the_features_and_check_alone_does_not()
-> TestResult {
    let steps = runs(&format!("{FEATURES}full_run = 'nightly'\n"))?;
    assert!(
        named(&steps, "Doctests")[0].contains("velnor-ci inputs build -- cargo test --doc"),
        "{steps:?}"
    );
    assert_featured(&steps, "Build every target", true);
    assert_featured(&steps, "Doctests", true);
    assert_featured(&steps, "Check package-0 alone", false);
    Ok(())
}

#[test]
fn a_helper_that_builds_apart_gives_the_no_run_build_and_the_separate_audit_the_features()
-> TestResult {
    let config = format!("{FEATURES}full_run = 'nightly'\n");
    let steps = planned(&["Cargo.toml", "lab/Cargo.toml"], &config, "0.1.101")?;
    let build = named(&steps, "Build the tests");
    assert!(
        !build.is_empty() && build.iter().all(|run| run.ends_with("--no-run")),
        "{build:?}"
    );
    assert_featured(&steps, "Build the tests", true);
    let audits = named(&steps, "Build every target");
    assert_eq!(audits.len(), 2, "{audits:?}");
    assert_featured(&steps, "Build every target", true);
    assert_featured(&steps, "Check package-0 alone", false);
    assert_featured(&steps, "Check package-1 alone", false);
    Ok(())
}

#[test]
fn an_unknown_or_invalid_workspace_feature_name_is_refused() -> TestResult {
    for names in ["['a b']", "['']", "['a', 'a']", "['a,b']", "['$x']"] {
        let config = format!("workspace_features = {names}\n");
        let repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        match detect(&repo) {
            Err(DetectError::Invalid {
                path,
                reason: Reason::InvalidValue { key, .. },
            }) => {
                assert_eq!((path, key), (CONFIG_PATH, "workspace_features"), "{names}");
            }
            other => return Err(format!("{names}: {other:?}").into()),
        }
    }
    Ok(())
}

const PACKAGE_AND_DEPENDENCY: &str =
    "[package]\nname = \"package-0\"\n[features]\nlocal-mode = []\n[dependencies]\nserde = \"1\"\n";

fn with_manifest(name: &str, manifest: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = repository(
        &["Cargo.toml"],
        &format!("workspace_features = ['{name}']\n"),
    )?;
    repo.files
        .insert(RepoPath::new("Cargo.toml")?, manifest.as_bytes().to_vec());
    Ok(repo)
}

fn refusal(repo: &RepoView) -> Result<(String, String), Box<dyn Error>> {
    match detect(repo) {
        Err(DetectError::Invalid {
            path,
            reason: Reason::UnknownWorkspaceFeature { root, feature },
        }) if path == CONFIG_PATH => Ok((root, feature)),
        other => Err(format!("{other:?}").into()),
    }
}

#[test]
fn a_root_with_no_package_that_defines_a_workspace_feature_is_refused_at_generate_time()
-> TestResult {
    let mut repo = repository(&["Cargo.toml", "lab/Cargo.toml"], FEATURES)?;
    repo.files.insert(
        RepoPath::new("lab/Cargo.toml")?,
        b"[package]\nname = \"package-1\"\n[features]\nother = []\n".to_vec(),
    );
    assert_eq!(refusal(&repo)?, ("lab".to_owned(), "local-mode".to_owned()));
    let message = workflow(&repo).err().ok_or("the repository was accepted")?;
    assert!(
        message.to_string().contains(
            "workspace_features names local-mode, which no package of the root lab defines"
        ),
        "{message}"
    );
    repo.files.remove(&RepoPath::new("Cargo.toml")?);
    assert_eq!(refusal(&repo)?, (".".to_owned(), "local-mode".to_owned()));
    Ok(())
}

#[test]
fn a_feature_an_optional_dependency_defines_is_accepted() -> TestResult {
    let repo = with_manifest(
        "serde",
        "[package]\nname = \"package-0\"\n[dependencies]\nserde = { version = \"1\", optional = true }\n",
    )?;
    detect(&repo)?;
    Ok(())
}

#[test]
fn a_package_feature_name_needs_a_member_that_defines_it_or_a_dependency_of_a_member() -> TestResult
{
    for name in ["package-0/local-modee", "ghost/local-mode"] {
        let repo = with_manifest(name, PACKAGE_AND_DEPENDENCY)?;
        assert_eq!(refusal(&repo)?, (".".to_owned(), name.to_owned()), "{name}");
    }
    for name in ["package-0/local-mode", "serde/derive"] {
        detect(&with_manifest(name, PACKAGE_AND_DEPENDENCY)?)?;
    }
    Ok(())
}

#[test]
fn a_test_lane_that_builds_with_the_workspace_features_is_refused() -> TestResult {
    let lane = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\n";
    let repo = repository(&["Cargo.toml"], &format!("{FEATURES}{lane}"))?;
    match detect(&repo) {
        Err(DetectError::Invalid {
            reason: Reason::TestLaneDuplicatesWorkspaceFeatures { features },
            ..
        }) => assert_eq!(features, "local-mode"),
        other => return Err(format!("{other:?}").into()),
    }
    let with_extra = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode', 'extra']\n";
    detect(&repository(
        &["Cargo.toml"],
        &format!("{FEATURES}{with_extra}"),
    )?)?;
    let alone_lane = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\n";
    detect(&repository(&["Cargo.toml"], alone_lane)?)?;
    Ok(())
}
