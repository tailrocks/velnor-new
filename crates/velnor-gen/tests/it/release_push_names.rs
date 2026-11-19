// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};
use velnor_release::ReleaseError;

use crate::yaml_tree::parse;

type TestResult = Result<(), Box<dyn Error>>;

const PATH: &str = ".github/workflows/extra.yml";

fn missing_name(text: &[u8], after: &str) -> Result<Option<String>, Box<dyn Error>> {
    let mut view = crate::release_fixture::release_view(
        &[crate::release_fixture::APP],
        Some(&format!("after = [{after}]\n")),
    )?;
    view.files.insert(RepoPath::new(PATH)?, text.to_vec());
    match expected_files(&view) {
        Ok(_) => Ok(None),
        Err(GenerateError::Release(ReleaseError::AfterMissingWorkflow { name })) => Ok(Some(name)),
        Err(other) => Err(other.into()),
    }
}

#[test]
fn a_push_workflow_the_old_name_scan_missed_must_be_named_in_after() -> TestResult {
    let bom = [b"\xEF\xBB\xBF".as_slice(), b"name: Extra\non: push\n"].concat();
    let cases: [(&[u8], &str); 7] = [
        (b"on: push\n", PATH),
        (b"name : Extra\non: push\n", "Extra"),
        (b"\"name\": Extra\non: push\n", "Extra"),
        (&bom, "Extra"),
        (b"name: >-\n  Extra\n  Name\non: push\n", "Extra Name"),
        (b"name: \"Extra\n  Name\"\non: push\n", "Extra Name"),
        (b"name: Extra\non: [push\n", PATH),
    ];
    for (text, expected) in cases {
        let shown = String::from_utf8_lossy(text);
        let name = missing_name(text, "\"Rust CI\"")?;
        assert_eq!(name.as_deref(), Some(expected), "{shown}");
        let after = format!("\"Rust CI\", {expected:?}");
        assert_eq!(missing_name(text, &after)?, None, "{shown}");
    }
    Ok(())
}

#[test]
fn a_workflow_without_a_name_key_is_not_a_push_workflow_unless_it_pushes() -> TestResult {
    assert_eq!(missing_name(b"on: pull_request\n", "\"Rust CI\"")?, None);
    Ok(())
}

#[test]
fn nested_push_templates_do_not_become_required_release_workflows() -> TestResult {
    const DIRECT: &str = ".github/workflows/ci.yml";
    const NESTED: &str = ".github/workflows/templates/push.yaml";
    let mut view = crate::release_fixture::release_view(
        &[crate::release_fixture::APP],
        Some(&format!("after = [\"Rust CI\", {DIRECT:?}]\n")),
    )?;
    view.files
        .insert(RepoPath::new(DIRECT)?, b"on: push\n".to_vec());
    view.files
        .insert(RepoPath::new(NESTED)?, b"on: push\n".to_vec());

    let files = expected_files(&view)?;
    let release = files
        .get(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release workflow")?;
    let tree = parse(release)?;
    assert_eq!(
        tree.at(&["on", "workflow_run", "workflows"])
            .ok_or("no workflow_run workflows")?
            .texts(),
        ["Rust CI", DIRECT]
    );
    Ok(())
}

#[test]
fn alias_dense_workflows_preserve_release_dependency_names_and_events() -> TestResult {
    for event in ["pull_request", "push"] {
        let mut text =
            format!("name: Extra\non: {event}\nenv:\n  SHARED: &shared 'released-helper'\n");
        for index in 0..200 {
            writeln!(text, "  COPY_{index}: *shared")?;
        }
        let expected = if event == "push" { Some("Extra") } else { None };
        assert_eq!(
            missing_name(text.as_bytes(), "\"Rust CI\"")?.as_deref(),
            expected
        );
        assert_eq!(
            missing_name(text.as_bytes(), "\"Rust CI\", \"Extra\"")?,
            None
        );
    }
    Ok(())
}

#[test]
fn workflows_beyond_input_or_expansion_bounds_stay_conservative() -> TestResult {
    let oversized = format!(
        "name: Extra\non: pull_request\nenv:\n  LONG: '{}'\n",
        "é".repeat(250_001)
    );
    let mut expanded = format!(
        "name: Extra\non: pull_request\nenv:\n  SHARED: &shared '{}'\n",
        "x".repeat(100_000)
    );
    for index in 0..200 {
        writeln!(expanded, "  COPY_{index}: *shared")?;
    }
    assert!(expanded.len() < 500_000);
    for text in [oversized, expanded] {
        assert_eq!(
            missing_name(text.as_bytes(), "\"Rust CI\"")?.as_deref(),
            Some(PATH)
        );
        assert_eq!(
            missing_name(text.as_bytes(), &format!("\"Rust CI\", {PATH:?}"))?,
            None
        );
    }
    Ok(())
}
