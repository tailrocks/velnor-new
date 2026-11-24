// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::github::rules_path;
use velnor_ci::merge_queue::{Context, queue_warning};

use crate::support::{FakeGitHub, repo};

fn context() -> Context {
    Context {
        event: "pull_request".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        base: "main".to_owned(),
    }
}

fn rules(kinds: &[&str]) -> String {
    let rules: Vec<_> = kinds.iter().map(|kind| json!({"type": kind})).collect();
    json!(rules).to_string()
}

fn fake(page: u32, body: &str) -> FakeGitHub {
    let mut fake = FakeGitHub::default();
    fake.respond(rules_path(&repo(), "main", page), body);
    fake
}

fn warned(github: &FakeGitHub, context: &Context) -> Result<String, String> {
    let line = queue_warning(github, context).ok_or("no warning")?;
    assert!(line.starts_with("::warning "), "{line}");
    assert!(!line.contains('\n'), "{line}");
    Ok(line)
}

#[test]
fn a_branch_without_a_queue_rule_warns_that_unverified_code_can_merge() -> Result<(), String> {
    let github = fake(1, &rules(&["pull_request", "required_status_checks"]));
    let line = warned(&github, &context())?;
    assert!(line.contains("unverified code can merge"), "{line}");
    assert!(line.contains("merge queue"), "{line}");
    assert!(line.contains("main"), "{line}");
    Ok(())
}

#[test]
fn a_branch_with_a_queue_rule_is_silent() {
    let github = fake(1, &rules(&["merge_queue"]));
    assert_eq!(queue_warning(&github, &context()), None);
}

#[test]
fn a_queue_rule_on_a_later_page_still_counts() {
    let mut github = fake(1, &rules(&vec!["pull_request"; 100]));
    github.respond(rules_path(&repo(), "main", 2), &rules(&["merge_queue"]));
    assert_eq!(queue_warning(&github, &context()), None);
}

#[test]
fn an_unreadable_rule_list_warns_that_the_check_failed_and_does_not_fail() -> Result<(), String> {
    let mut missing = FakeGitHub::default();
    missing.missing(rules_path(&repo(), "main", 1));
    for github in [
        FakeGitHub::default(),
        missing,
        fake(1, "not json"),
        fake(
            1,
            r#"{"message": "Resource not accessible by integration"}"#,
        ),
    ] {
        let line = warned(&github, &context())?;
        assert!(line.contains("Could not check"), "{line}");
        assert!(!line.contains("unverified code can merge"), "{line}");
    }
    Ok(())
}

#[test]
fn only_a_pull_request_run_asks_the_api() {
    for event in ["push", "merge_group", "schedule", "workflow_dispatch"] {
        let context = Context {
            event: event.to_owned(),
            ..context()
        };
        assert_eq!(
            queue_warning(&FakeGitHub::default(), &context),
            None,
            "{event}"
        );
    }
}

#[test]
fn a_pull_request_run_with_an_unreadable_context_warns_instead_of_failing() -> Result<(), String> {
    let github = fake(1, &rules(&["merge_queue"]));
    for edit in [
        |c: &mut Context| c.base = String::new(),
        |c: &mut Context| c.repository = "not a repository".to_owned(),
    ] {
        let mut context = context();
        edit(&mut context);
        assert!(warned(&github, &context)?.contains("Could not check"));
    }
    Ok(())
}

#[test]
fn a_branch_name_cannot_inject_a_workflow_command() -> Result<(), String> {
    let context = Context {
        base: "x\n::error::pwned".to_owned(),
        ..context()
    };
    let line = warned(&FakeGitHub::default(), &context)?;
    assert!(!line.contains('\n'), "{line}");
    assert!(line.contains("%0A::error::pwned"), "{line}");
    Ok(())
}
