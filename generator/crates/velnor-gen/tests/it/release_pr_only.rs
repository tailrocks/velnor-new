// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::release_fixture::{APP, LOCK, release_view};
use crate::yaml_tree::parse;
use std::error::Error;
use velnor_gen::expected_files;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

type TestResult = Result<(), Box<dyn Error>>;

fn render(helper: &str, config: &str) -> Result<String, Box<dyn Error>> {
    let mut view = release_view(&[APP], Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", helper).into_bytes());
    expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or_else(|| "no release.yml".into())
}

#[test]
fn absent_and_empty_pr_only_checks_preserve_release_output() -> TestResult {
    assert_eq!(
        render("0.1.81", "")?,
        render("0.1.81", "pr_only_checks = []\n")?
    );
    Ok(())
}

#[test]
fn pr_only_checks_are_explicit_literal_repeated_flags_in_the_release_gate() -> TestResult {
    let text = render("0.1.81", "pr_only_checks = [\"DCO\", \"-team's check\"]\n")?;
    assert!(parse(&text)?.scalars().contains(&"velnor-ci release-gate --repo \"$REPOSITORY\" --sha \"$COMMIT\" --pr-only-check=DCO --pr-only-check='-team'\\''s check'"));
    Ok(())
}

#[test]
fn pr_only_checks_refuse_workflow_expressions() {
    let error = render("0.1.81", "pr_only_checks=['${{x}}']")
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        error.contains("pr_only_checks") && error.contains("${{"),
        "{error}"
    );
}
