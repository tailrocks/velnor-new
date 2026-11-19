// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{GroupRoot, RepoPath, TestNeed};
use velnor_rust::detect;

use crate::facts::TestResult;
use crate::pipeline::{HELPER_LOCK, view, workflow};
use crate::yaml_tree::document;
use serde_json::Value;

pub(crate) const DATABASE: &str = "[[test_groups]]
name = 'database'
filter = 'package(package-0) and test(/^db_/)'
needs = ['docker']
test_threads = 2
timeout_minutes = 45
runner = 'ubuntu-24.04'
";

pub(crate) const SHELL: &str = "[[test_groups]]
name = 'shell'
filter = 'binary(it) and test(~cargo_)'
needs = ['cargo']
";

pub(crate) fn view_with(
    roots: &[&str],
    config: &str,
    helper: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut view = view(roots, Some(config.as_bytes()))?;
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.81", helper, 1);
    view.files
        .insert(RepoPath::new(".velnor/rust-ci.lock")?, lock.into_bytes());
    Ok(view)
}

pub(crate) fn rendered(config: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let text = workflow(&view_with(&["Cargo.toml"], config, "0.1.82")?)?;
    Ok(document(&text)?)
}

pub(crate) fn rejection(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let error = workflow(&view_with(
        &["Cargo.toml", "lab/Cargo.toml"],
        config,
        "0.1.82",
    )?)
    .err()
    .ok_or("the configuration was accepted")?;
    Ok(error.to_string())
}

pub(crate) fn job<'a>(workflow: &'a Value, id: &str) -> Result<&'a Value, String> {
    let job = &workflow["jobs"][id];
    if job.is_null() {
        Err(format!("no job {id}"))
    } else {
        Ok(job)
    }
}

pub(crate) fn steps(job: &Value) -> &[Value] {
    job["steps"].as_array().map_or(&[], Vec::as_slice)
}

pub(crate) fn names(job: &Value) -> Vec<&str> {
    steps(job)
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect()
}

pub(crate) fn step<'a>(job: &'a Value, name: &str) -> Result<&'a Value, String> {
    steps(job)
        .iter()
        .find(|step| step["name"] == name)
        .ok_or_else(|| format!("no step {name}"))
}

pub(crate) fn run_of<'a>(job: &'a Value, name: &str) -> Result<&'a str, String> {
    step(job, name)?["run"]
        .as_str()
        .ok_or_else(|| format!("{name} has no run"))
}

pub(crate) fn needs(job: &Value) -> Vec<&str> {
    job["needs"].as_array().map_or_else(Vec::new, |needs| {
        needs.iter().filter_map(Value::as_str).collect()
    })
}

#[test]
fn the_test_groups_table_parses_every_field() -> TestResult {
    let both = format!("{DATABASE}\n{SHELL}");
    let facts = detect(&view_with(&["Cargo.toml"], &both, "0.1.82")?)?;
    let groups = &facts.config.test_groups;
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name.as_str(), "database");
    assert_eq!(groups[0].filter, "package(package-0) and test(/^db_/)");
    assert_eq!(groups[0].needs, [TestNeed::Docker]);
    assert_eq!(groups[0].test_threads, Some(2));
    assert_eq!(groups[0].timeout_minutes, Some(45));
    assert_eq!(
        groups[0].runner.as_ref().map(ToString::to_string),
        Some("ubuntu-24.04".to_owned())
    );
    assert_eq!(groups[0].root, GroupRoot::OnlyRoot);
    assert_eq!(groups[1].needs, [TestNeed::Cargo]);
    assert_eq!(groups[1].test_threads, None);
    assert_eq!(groups[1].runner, None);
    Ok(())
}

#[test]
fn a_group_names_its_root_by_folder_or_dot() -> TestResult {
    let config = "[[test_groups]]\nname = 'slow'\nfilter = 'all()'\nroot = 'lab'\n\n[[test_groups]]\nname = 'top'\nfilter = 'all()'\nroot = '.'\n";
    let facts = detect(&view_with(
        &["Cargo.toml", "lab/Cargo.toml"],
        config,
        "0.1.82",
    )?)?;
    assert_eq!(
        facts.config.test_groups[0].root,
        GroupRoot::Folder(Some(RepoPath::new("lab")?))
    );
    assert_eq!(facts.config.test_groups[1].root, GroupRoot::Folder(None));
    Ok(())
}

#[test]
fn an_unknown_key_an_empty_group_or_a_bad_value_is_refused_with_its_name() -> TestResult {
    let group = |body: &str| format!("[[test_groups]]\nname = 'db'\nroot = '.'\n{body}");
    for (config, mention) in [
        (group("filter = 'all()'\ncolor = 'red'\n"), "color"),
        (group(""), "filter"),
        (group("filter = '   '\n"), "test_groups.filter"),
        (group("filter = 'all()'\nneeds = ['gpu']\n"), "gpu"),
        (
            group("filter = 'all()'\nneeds = ['docker', 'docker']\n"),
            "test_groups.needs",
        ),
        (
            group("filter = 'all()'\ntest_threads = 0\n"),
            "test_threads",
        ),
        (
            group("filter = 'all()'\ntest_threads = 257\n"),
            "test_threads",
        ),
        (
            group("filter = 'all()'\ntimeout_minutes = 0\n"),
            "timeout_minutes",
        ),
        (group("filter = 'all()'\nevents = ['tuesday']\n"), "events"),
        (
            "[[test_groups]]\nname = 'db'\nfilter = 'all()'\nroot = 'missing'\n".to_owned(),
            "missing",
        ),
        (
            "[[test_groups]]\nname = 'default'\nfilter = 'all()'\nroot = '.'\n".to_owned(),
            "test_groups.name",
        ),
        (
            "[[test_groups]]\nname = 'db'\nfilter = 'all()'\n".to_owned(),
            "test_groups.root",
        ),
    ] {
        let message = rejection(&config)?;
        assert!(message.contains(mention), "{config}\n{message}");
    }
    Ok(())
}

#[test]
fn a_filter_that_needs_shell_syntax_is_refused_and_names_the_words_to_use() -> TestResult {
    for filter in [
        "package(app) & test(/^db_/)",
        "package(app) | package(lib)",
        "test(a); test(b)",
        "test(\"a\")",
        "test($HOME)",
        "test(`id`)",
        "test(a)\\ntest(b)",
        "if",
    ] {
        let config =
            format!("[[test_groups]]\nname = 'db'\nroot = '.'\nfilter = \"\"\"{filter}\"\"\"\n");
        let message = rejection(&config)?;
        assert!(
            message.contains("test_groups.filter") && message.contains("and, or and not"),
            "{filter}: {message}"
        );
    }
    Ok(())
}

#[test]
fn two_groups_with_the_same_name_in_one_root_are_refused() -> TestResult {
    let config =
        format!("{DATABASE}\n{DATABASE}").replace("[[test_groups]]", "[[test_groups]]\nroot = '.'");
    let message = rejection(&config)?;
    assert!(
        message.contains("test_groups.name") && message.contains("database"),
        "{message}"
    );
    Ok(())
}

#[test]
fn a_pinned_helper_older_than_the_groups_release_refuses_the_table() -> TestResult {
    let error = workflow(&view_with(&["Cargo.toml"], DATABASE, "0.1.81")?)
        .err()
        .ok_or("an old helper was accepted")?
        .to_string();
    assert!(
        error.contains("0.1.81") && error.contains("0.1.82") && error.contains("test_groups"),
        "{error}"
    );
    workflow(&view_with(&["Cargo.toml"], "", "0.1.81")?)?;
    Ok(())
}
