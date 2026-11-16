// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::{PackageName, RepoPath};
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, Root, release_plz, release_view, render};
use crate::release_yml_binaries::job;
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const PUBLISHING: Root = Root {
    publishes: true,
    ..APP
};

fn packages(workspace: &str, packages: &[(&str, &str)]) -> String {
    let mut text = release_plz(&PUBLISHING)
        .split("\n[[package]]")
        .next()
        .unwrap_or_default()
        .replace("publish_timeout = \"5m\"\n", "")
        .replace("[workspace]\n", &format!("[workspace]\n{workspace}"));
    for (name, timeout) in packages {
        text.extend(["\n[[package]]\nname = \"", name, "\"\n"]);
        if !timeout.is_empty() {
            text.extend(["publish_timeout = \"", timeout, "\"\n"]);
        }
    }
    text
}

fn with_packages(
    text: &str,
    names: &[&str],
) -> Result<Result<Node, GenerateError>, Box<dyn Error>> {
    let mut view = release_view(&[PUBLISHING], Some("roots = [\".\"]\n"))?;
    let metadata = view
        .rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no metadata")?;
    let first = metadata.members[0].clone();
    for name in names {
        let mut member = first.clone();
        member.name = PackageName::new(*name)?;
        metadata.members.push(member);
    }
    view.files
        .insert(RepoPath::new("release-plz.toml")?, text.as_bytes().to_vec());
    match expected_files(&view) {
        Ok(mut files) => {
            let text = files
                .remove(&velnor_model::RELEASE_WORKFLOW_PATH)
                .ok_or("no release.yml")?;
            Ok(Ok(crate::yaml_tree::parse(&text)?))
        }
        Err(error) => Ok(Err(error)),
    }
}

fn with_timeout(value: &str) -> Result<Result<Node, GenerateError>, Box<dyn Error>> {
    with_packages(
        &packages(&format!("publish_timeout = \"{value}\"\n"), &[("app", "")]),
        &[],
    )
}

fn minutes(tree: &Node) -> Result<Option<String>, Box<dyn Error>> {
    Ok(job(tree, "tag")?
        .get("timeout-minutes")
        .and_then(Node::text)
        .map(str::to_owned))
}

#[test]
fn the_tag_job_is_sized_for_the_registry_check_and_the_publish_of_every_published_package()
-> TestResult {
    let (_, default) = render(&[PUBLISHING], Some("roots = [\".\"]\n"))?;
    assert_eq!(minutes(&default)?.as_deref(), Some("40"));
    for (value, total) in [("10m", "50"), ("20m", "70"), ("90s", "34")] {
        let tree = with_timeout(value)??;
        assert_eq!(minutes(&tree)?.as_deref(), Some(total), "{value}");
    }
    Ok(())
}

fn outlives(result: Result<Node, GenerateError>) -> Option<u64> {
    match result.err() {
        Some(GenerateError::Release(ReleaseError::PublishBudgetOutlivesToken {
            minutes,
            limit: 50,
        })) => Some(minutes),
        _ => None,
    }
}

#[test]
fn a_package_without_a_publish_timeout_is_budgeted_at_release_plzs_default_of_thirty_minutes_twice()
-> TestResult {
    let text = packages("", &[("app", "1m"), ("second", "")]);
    assert_eq!(outlives(with_packages(&text, &["second"])?), Some(62));
    Ok(())
}

#[test]
fn each_package_is_budgeted_at_its_own_effective_timeout_not_the_largest_one_times_the_count()
-> TestResult {
    let text = packages(
        "publish_timeout = \"60m\"\n",
        &[
            ("app", "1m"),
            ("b", "1m"),
            ("c", "1m"),
            ("d", "1m"),
            ("e", "1m"),
            ("f", "1m"),
        ],
    );
    let tree = with_packages(&text, &["b", "c", "d", "e", "f"])??;
    assert_eq!(minutes(&tree)?.as_deref(), Some("42"));
    Ok(())
}

#[test]
fn a_package_that_does_not_publish_is_still_budgeted_for_its_registry_check() -> TestResult {
    let unpublished = |timeouts: &[(&str, &str)]| packages("publish = false\n", timeouts);
    let tree = with_packages(&unpublished(&[("app", "25m")]), &[])??;
    assert_eq!(minutes(&tree)?.as_deref(), Some("55"));
    let two = unpublished(&[("app", ""), ("second", "")]);
    assert_eq!(outlives(with_packages(&two, &["second"])?), Some(60));
    let git_only = packages("git_only = true\n", &[("app", "")]);
    let tree = with_packages(&git_only, &[])??;
    assert_eq!(minutes(&tree)?.as_deref(), Some("30"));
    Ok(())
}

#[test]
fn a_budget_that_outlives_the_one_hour_app_token_is_refused() -> TestResult {
    for (value, total) in [("1h 30m", 180), ("26m", 52)] {
        assert_eq!(outlives(with_timeout(value)?), Some(total), "{value}");
    }
    assert!(with_timeout("25m")?.is_ok());
    let two_defaults = packages("", &[("app", ""), ("second", "")]);
    assert_eq!(
        outlives(with_packages(&two_defaults, &["second"])?),
        Some(120)
    );
    Ok(())
}

#[test]
fn a_publish_timeout_that_is_not_a_duration_is_refused() -> TestResult {
    assert!(matches!(
        with_timeout("soon")?.err(),
        Some(GenerateError::Release(
            ReleaseError::InvalidPublishTimeout { .. }
        ))
    ));
    Ok(())
}

#[test]
fn a_release_workflow_above_the_github_file_size_limit_is_refused() -> TestResult {
    let roots: Vec<Root> = (0..60)
        .map(|index| Root {
            dir: Box::leak(format!("crate-{index}").into_boxed_str()),
            package: Box::leak(format!("package-{index}").into_boxed_str()),
            prefix: Box::leak(format!("release-plz-{index}-").into_boxed_str()),
            publishes: false,
        })
        .collect();
    let list: Vec<String> = roots
        .iter()
        .map(|root| format!("\"{}\"", root.dir))
        .collect();
    let binaries: Vec<String> = roots
        .iter()
        .map(|root| format!("[[binaries]]\npackage = \"{}\"\n", root.package))
        .collect();
    let config = format!("roots = [{}]\n{}", list.join(", "), binaries.concat());
    let error = expected_files(&release_view(&roots, Some(&config))?).err();
    let Some(GenerateError::Rules(violations)) = error else {
        return Err("rendered".into());
    };
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation.rule, velnor_render::Rule::WorkflowTooLarge(_)))
    );
    assert!(violations[0].to_string().contains("above 500000 bytes"));
    Ok(())
}
