// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_group_partitions::{HELPER, group, refused, rendered_with};
use crate::test_groups::view_with;

fn one(name: &str, path: &str) -> String {
    group(&format!(
        "artifacts = [{{ name = '{name}', paths = ['{path}'], when = 'failure' }}]\n"
    ))
}

#[test]
fn a_glob_that_can_expand_to_a_build_directory_is_refused() -> TestResult {
    for path in [
        "targ*/**",
        "target/deb*/**",
        "target/*bug/**",
        "target/rel*",
        "target/*/",
        "app/t*get/",
        "*/release/**",
        "*/debug",
        "Target/debug",
    ] {
        let message = refused(&one("a", path), HELPER)?;
        assert!(
            message.contains("test_groups.artifacts.paths"),
            "{path:?}: {message}"
        );
    }
    for path in [
        "target/nextest/*/junit.xml",
        "target/nextest/ci/*.xml",
        "target/debug/report.xml",
        "target/release/*.xml",
        "tests/*/release.xml",
        "reports/*/*.xml",
    ] {
        rendered_with(&one("a", path), HELPER)?;
    }
    Ok(())
}

#[test]
fn a_double_star_that_follows_only_wildcard_parts_is_refused_as_a_whole_workspace_walk()
-> TestResult {
    for path in ["*/**/*.log", "*/*/**/x.xml", "*/**", "**/*.log"] {
        let message = refused(&one("a", path), HELPER)?;
        assert!(
            message.contains("test_groups.artifacts.paths"),
            "{path:?}: {message}"
        );
    }
    for path in ["reports/**/*.log", "*/reports/**/x.xml"] {
        rendered_with(&one("a", path), HELPER)?;
    }
    Ok(())
}

#[test]
fn the_ci_metrics_name_prefix_belongs_to_the_generated_report_jobs() -> TestResult {
    for name in ["ci-metrics-db", "CI-Metrics-x", "ci-metrics-"] {
        let message = refused(&one(name, "junit.xml"), HELPER)?;
        assert!(
            message.contains("test_groups.artifacts.name") && message.contains("ci-metrics-"),
            "{name:?}: {message}"
        );
    }
    rendered_with(&one("ci-metrics", "junit.xml"), HELPER)?;
    let split = |name: &str| {
        group(&format!(
            "partitions = 2\nartifacts = [{{ name = '{name}', paths = ['junit.xml'], when = 'failure' }}]\n"
        ))
    };
    for name in ["ci-metrics", "velnor"] {
        let message = refused(&split(name), HELPER)?;
        assert!(message.contains(&format!("{name}-1")), "{name}: {message}");
    }
    rendered_with(&split("nextest"), HELPER)?;
    Ok(())
}

fn sparse_lists(config: &str) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let text = workflow(&view_with(
        &["a/Cargo.toml", "b/Cargo.toml"],
        config,
        HELPER,
    )?)?;
    Ok(jobs_of(&text)
        .into_iter()
        .filter(|(id, _)| id.starts_with("rust-test-database"))
        .map(|(_, block)| {
            block
                .split("sparse-checkout: |\n")
                .nth(1)
                .unwrap_or_default()
                .lines()
                .take_while(|line| line.starts_with("            "))
                .map(|line| line.trim().to_owned())
                .collect()
        })
        .collect())
}

#[test]
fn a_group_artifact_path_outside_the_cargo_root_is_in_the_sparse_checkout() -> TestResult {
    let config = |artifacts: &str| {
        format!(
            "full_run = \"off\"\nsparse_checkout = true\n[[test_groups]]\nname = 'database'\nroot = 'a'\nfilter = 'all()'\n{artifacts}"
        )
    };
    let with = config(
        "artifacts = [{ name = 'base', paths = ['reports/baseline.xml', 'snaps/**/*.snap', 'out/'], when = 'always' }]\n",
    );
    let lists = sparse_lists(&with)?;
    assert!(!lists.is_empty() && lists.iter().all(|list| !list.is_empty()));
    for list in &lists {
        for wanted in ["/reports/baseline.xml", "/snaps/**/*.snap", "/out/"] {
            assert!(list.iter().any(|line| line == wanted), "{wanted}: {list:?}");
        }
        assert!(list.iter().any(|line| line == "/a/"), "{list:?}");
    }
    for list in sparse_lists(&config(""))? {
        assert!(
            !list.iter().any(|line| line.contains("reports")),
            "{list:?}"
        );
    }
    Ok(())
}
