// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use serde_json::Value;
use velnor_model::{RepoPath, RepoView};
use velnor_rust::{LOCK_PATH, detect};

use crate::facts::TestResult;
use crate::golden::{load, sample_dirs, samples};
use crate::pipeline::{pin_helper, view, workflow};

const WORKSPACE: &str = "${{ github.workspace }}/";
/// The first helper whose roots outside the repository root build below `target/velnor-roots`.
const NESTED_HELPER: &str = "0.1.107";

fn pin_version(view: &mut RepoView, version: &str) -> Result<(), Box<dyn Error>> {
    let bytes = view.files.get(&LOCK_PATH).ok_or("no helper lock")?;
    let text = String::from_utf8(bytes.clone())?;
    let pinned: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with("version = ") {
                format!("version = \"{version}\"")
            } else {
                line.to_owned()
            }
        })
        .collect();
    view.files
        .insert(LOCK_PATH, format!("{}\n", pinned.join("\n")).into_bytes());
    Ok(())
}

fn nested_fixture() -> Result<RepoView, Box<dyn Error>> {
    let mut repo = view(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/c/Cargo.toml"],
        None,
    )?;
    pin_helper(&mut repo, NESTED_HELPER)?;
    Ok(repo)
}

/// The directories of every package of every root, except the repository root itself, whose
/// own `target` folder is where Cargo puts it by default.
fn package_dirs(view: &RepoView) -> BTreeSet<RepoPath> {
    view.rust_metadata
        .iter()
        .flat_map(|(workspace, metadata)| {
            std::iter::once(workspace.clone()).chain(
                metadata
                    .members
                    .iter()
                    .map(|member| member.manifest.clone()),
            )
        })
        .filter_map(|manifest| manifest.parent())
        .collect()
}

fn inside(path: &RepoPath, folder: &RepoPath) -> bool {
    path == folder || path.as_str().starts_with(&format!("{folder}/"))
}

fn assert_outside_every_package(name: &str, view: &RepoView) -> TestResult {
    let facts = detect(view)?;
    let packages = package_dirs(view);
    for root in &facts.rust_roots {
        for package in &packages {
            assert!(
                !inside(&root.target_dir, package),
                "{name}: the target {} of {} lies inside the package {package}",
                root.target_dir,
                root.manifest
            );
        }
    }
    Ok(())
}

#[test]
fn a_nested_root_builds_below_the_repository_target_folder_outside_every_package() -> TestResult {
    let repo = nested_fixture()?;
    assert_outside_every_package("fixture", &repo)?;
    let facts = detect(&repo)?;
    let targets: Vec<(String, String)> = facts
        .rust_roots
        .iter()
        .map(|root| (root.manifest.to_string(), root.target_dir.to_string()))
        .collect();
    assert_eq!(
        targets,
        [
            ("Cargo.toml".to_owned(), "target".to_owned()),
            (
                "tools/a/Cargo.toml".to_owned(),
                "target/velnor-roots/tools_a".to_owned()
            ),
            (
                "tools/b/c/Cargo.toml".to_owned(),
                "target/velnor-roots/tools_b_c".to_owned()
            ),
        ]
    );
    Ok(())
}

#[test]
fn no_root_of_any_sample_builds_inside_a_package_directory_from_the_new_helper() -> TestResult {
    let mut nested = 0;
    for sample in sample_dirs(&samples())? {
        let name = sample.file_name().unwrap_or_default().to_string_lossy();
        let mut repo = load(&sample)?;
        if !repo.files.contains_key(&LOCK_PATH) {
            continue;
        }
        pin_version(&mut repo, NESTED_HELPER)?;
        let facts = detect(&repo)?;
        nested += facts
            .rust_roots
            .iter()
            .filter(|root| root.manifest.parent().is_some())
            .count();
        assert_outside_every_package(&name, &repo)?;
    }
    assert!(
        nested > 0,
        "no sample has a root outside the repository root"
    );
    Ok(())
}

#[test]
fn a_repository_root_workspace_keeps_its_target_folder_for_every_helper() -> TestResult {
    for helper in ["0.1.93", NESTED_HELPER] {
        let mut repo = view(&["Cargo.toml"], None)?;
        pin_helper(&mut repo, helper)?;
        let facts = detect(&repo)?;
        assert_eq!(
            facts.rust_roots[0].target_dir.as_str(),
            "target",
            "{helper}"
        );
    }
    Ok(())
}

#[test]
fn a_helper_that_predates_the_nested_target_rule_keeps_the_folder_beside_the_manifest() -> TestResult
{
    let mut repo = view(&["Cargo.toml", "tools/a/Cargo.toml"], None)?;
    pin_helper(&mut repo, "0.1.106")?;
    let facts = detect(&repo)?;
    assert_eq!(facts.rust_roots[1].target_dir.as_str(), "tools/a/target");
    Ok(())
}

fn env_targets(job: &Value) -> BTreeSet<String> {
    let steps = job["steps"].as_array().map_or(&[][..], Vec::as_slice);
    std::iter::once(&job["env"])
        .chain(steps.iter().map(|step| &step["env"]))
        .filter_map(|env| env["CARGO_TARGET_DIR"].as_str())
        .map(|value| value.trim_start_matches(WORKSPACE).to_owned())
        .collect()
}

fn cache_paths(job: &Value) -> BTreeSet<String> {
    let steps = job["steps"].as_array().map_or(&[][..], Vec::as_slice);
    steps
        .iter()
        .filter(|step| {
            step["name"]
                .as_str()
                .is_some_and(|name| name.ends_with("the target cache"))
        })
        .filter_map(|step| step["with"]["path"].as_str().map(str::to_owned))
        .collect()
}

fn helper_arguments(job: &Value, flag: &str) -> BTreeSet<String> {
    let steps = job["steps"].as_array().map_or(&[][..], Vec::as_slice);
    steps
        .iter()
        .filter_map(|step| step["run"].as_str())
        .flat_map(|run| {
            let words: Vec<&str> = run.split_whitespace().collect();
            words
                .windows(2)
                .filter(|pair| pair[0] == flag)
                .map(|pair| pair[1].trim_matches('\'').to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A helper argument as a path from the repository root: absolute under the workspace, or
/// relative to the root's folder.
fn from_workspace(argument: &str) -> String {
    let argument = argument.replace("\"${GITHUB_WORKSPACE}\"/", "");
    argument.trim_start_matches("../").to_owned()
}

#[test]
fn save_restore_trim_and_cargo_all_name_the_same_target_of_a_nested_root() -> TestResult {
    let yaml: Value = serde_saphyr::from_str(&workflow(&nested_fixture()?)?)?;
    let every_root: BTreeSet<String> = [
        "target",
        "target/velnor-roots/tools_a",
        "target/velnor-roots/tools_b_c",
    ]
    .map(str::to_owned)
    .into();
    let mut checked = BTreeSet::new();
    for (id, job) in yaml["jobs"].as_object().ok_or("jobs")? {
        let cargo = env_targets(job);
        let [target] = cargo.iter().collect::<Vec<_>>()[..] else {
            continue;
        };
        if !target.starts_with("target/velnor-roots/") {
            continue;
        }
        assert_eq!(&cache_paths(job), &cargo, "{id}: the cache path");
        let own: BTreeSet<String> = helper_arguments(job, "--target-dir")
            .iter()
            .map(|argument| from_workspace(argument))
            .collect();
        assert!(own.contains(target.as_str()), "{id}: {own:?}");
        assert!(own.is_subset(&every_root), "{id}: {own:?}");
        let staging: BTreeSet<String> = helper_arguments(job, "--staging-dir");
        assert_eq!(
            staging,
            BTreeSet::from([format!("{target}.velnor-delta")]),
            "{id}: the delta layer"
        );
        checked.insert(target.clone());
    }
    assert_eq!(
        checked,
        BTreeSet::from([
            "target/velnor-roots/tools_a".to_owned(),
            "target/velnor-roots/tools_b_c".to_owned()
        ])
    );
    Ok(())
}
