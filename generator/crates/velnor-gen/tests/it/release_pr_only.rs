// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::release_fixture::{APP, LOCK, release_view};
use crate::yaml_tree::{Node, parse};
use std::error::Error;
use velnor_gen::expected_files;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

type TestResult = Result<(), Box<dyn Error>>;

fn render(helper: &str, config: &str) -> Result<String, Box<dyn Error>> {
    let mut view = release_view(&[APP], Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", helper).into_bytes());
    expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or_else(|| "no release.yml".into())
}

#[test]
fn absent_and_empty_pr_only_checks_preserve_release_output_for_old_and_new_helpers() -> TestResult {
    for helper in ["0.1.80", "0.1.81", "0.1.82"] {
        let tree = parse(&render(helper, "")?)?;
        let gate = tree.at(&["jobs", "gate"]).ok_or("no gate")?;
        assert!(gate.get("environment").is_none());
        assert!(gate.scalars().contains(&"${{ github.token }}"));
        assert!(!gate.scalars().contains(&"${{ steps.app.outputs.token }}"));
        assert!(!gate.scalars().contains(&"app"));
        assert_eq!(
            render(helper, "")?,
            render(helper, "pr_only_checks = []\n")?
        );
    }
    Ok(())
}

#[test]
fn pr_only_checks_are_explicit_literal_repeated_flags_in_the_release_gate() -> TestResult {
    let text = render("0.1.82", "pr_only_checks = [\"DCO\", \"-team's check\"]\n")?;
    let tree = parse(&text)?;
    assert!(tree.scalars().contains(&"velnor-ci release-gate --repo \"$REPOSITORY\" --sha \"$COMMIT\" --pr-only-check=DCO --pr-only-check='-team'\\''s check'"));
    let permissions = tree
        .at(&["jobs", "gate", "permissions"])
        .ok_or("no gate permissions")?;
    assert_eq!(
        permissions.keys(),
        ["actions", "contents", "statuses", "checks"]
    );
    for scope in permissions.keys() {
        assert_eq!(permissions.get(scope).and_then(Node::text), Some("read"));
    }
    let gate = tree.at(&["jobs", "gate"]).ok_or("no gate")?;
    let steps = gate.get("steps").ok_or("no gate steps")?.items();
    let app = steps
        .iter()
        .find(|step| step.get("id").and_then(Node::text) == Some("app"))
        .ok_or("exemptions require a release App token")?;
    assert_eq!(
        app.get("uses").and_then(Node::text),
        Some("actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510")
    );
    for (input, value) in [
        ("client-id", "${{ secrets.RELEASE_APP_ID }}"),
        ("private-key", "${{ secrets.RELEASE_APP_PRIVATE_KEY }}"),
        ("owner", "${{ github.repository_owner }}"),
        ("repositories", "${{ github.event.repository.name }}"),
        ("permission-administration", "read"),
        ("permission-actions", "read"),
        ("permission-contents", "read"),
        ("permission-statuses", "read"),
        ("permission-checks", "read"),
    ] {
        assert_eq!(app.at(&["with", input]).and_then(Node::text), Some(value));
    }
    assert_eq!(
        gate.get("environment").and_then(Node::text),
        Some("release")
    );
    let check = steps
        .iter()
        .find(|step| step.get("id").and_then(Node::text) == Some("gate"))
        .ok_or("no gate command")?;
    assert_eq!(
        check.at(&["env", "GITHUB_TOKEN"]).and_then(Node::text),
        Some("${{ steps.app.outputs.token }}")
    );
    assert!(steps.iter().all(|step| {
        step.get("uses")
            .and_then(Node::text)
            .is_none_or(|uses| !uses.starts_with("actions/checkout@"))
            && step
                .get("run")
                .and_then(Node::text)
                .is_none_or(|run| run.starts_with("velnor-ci release-gate "))
    }));
    Ok(())
}

#[test]
fn pr_only_checks_refuse_old_helpers_and_workflow_expressions() {
    for (helper, config, reason) in [
        ("0.1.80", "pr_only_checks=['DCO']", "pin-helper 0.1.82"),
        ("0.1.81", "pr_only_checks=['DCO']", "pin-helper 0.1.82"),
        ("0.1.82", "pr_only_checks=['${{x}}']", "${{"),
    ] {
        let error = render(helper, config)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("pr_only_checks") && error.contains(reason),
            "{error}"
        );
    }
}
