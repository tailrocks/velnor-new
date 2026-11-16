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
    let text = packages("", &[("app", ""), ("second", "")]);
    assert_eq!(outlives(with_packages(&text, &["second"])?), Some(120));
    Ok(())
}

#[test]
fn the_workspace_publish_timeout_is_charged_twice_for_every_publishing_package() -> TestResult {
    let workspace = "publish_timeout = \"10m\"\n";
    let two = packages(workspace, &[("app", ""), ("second", "")]);
    assert!(with_packages(&two, &["second"])?.is_ok());
    let three = packages(workspace, &[("app", ""), ("b", ""), ("c", "")]);
    assert_eq!(outlives(with_packages(&three, &["b", "c"])?), Some(60));
    Ok(())
}

#[test]
fn a_package_level_publish_timeout_is_refused_because_release_plz_reads_only_the_workspace_one()
-> TestResult {
    let text = packages("publish_timeout = \"60m\"\n", &[("app", "1m")]);
    assert!(matches!(
        with_packages(&text, &[])?.err(),
        Some(GenerateError::Release(
            ReleaseError::PackagePublishTimeout { .. }
        ))
    ));
    Ok(())
}

#[test]
fn a_package_that_does_not_publish_costs_no_registry_budget() -> TestResult {
    let unpublished = packages("publish = false\n", &[("app", ""), ("second", "")]);
    assert!(with_packages(&unpublished, &["second"])?.is_ok());
    let git_only = packages("git_only = true\n", &[("app", ""), ("second", "")]);
    assert!(with_packages(&git_only, &["second"])?.is_ok());
    let mixed = packages(
        "publish_timeout = \"25m\"\n",
        &[("app", ""), ("second", "")],
    );
    let mixed = mixed.replace(
        "name = \"second\"\n",
        "name = \"second\"\npublish = false\n",
    );
    assert!(with_packages(&mixed, &["second"])?.is_ok());
    Ok(())
}

#[test]
fn an_inherited_package_without_a_package_entry_is_budgeted_at_the_default_twice() -> TestResult {
    let text = packages("", &[]);
    assert_eq!(outlives(with_packages(&text, &[])?), Some(60));
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
    // Every root and package adds to the check-config and tag-pattern runs, which
    // are capped at 2000 characters, so many roots hit that cap before they reach
    // the file limit. The trigger's `after` list grows the file without a run.
    let names: Vec<String> = (0..100)
        .map(|index| format!("Workflow {index:03} {}", "w".repeat(6000)))
        .collect();
    let roots = [APP];
    let config = format!(
        "roots = [\".\"]\nafter = [\"Rust CI\", {}]\n[[binaries]]\npackage = \"app\"\n",
        names
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut view = release_view(&roots, Some(&config))?;
    for (index, name) in names.iter().enumerate() {
        view.files.insert(
            RepoPath::new(format!(".github/workflows/after-{index}.yml"))?,
            format!("name: {name}\non: push\n").into_bytes(),
        );
    }
    let error = expected_files(&view).err();
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
