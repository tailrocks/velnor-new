// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_calls::CONFIG;
use crate::declared_fixture::{JOB, TestResult, job, refusal, rendered, rendered_file, workflow};
use crate::yaml_tree::Node;

const PUSH: &str = "[workflow.on.push]\nbranches = [\"main\"]";
const PULL_REQUEST: &str = "[workflow.on.pull_request]\ntypes = [\"opened\"]";
const MERGE_GROUP: &str = "[workflow.on]\nmerge_group = true";
const LOCAL: &str = "./.github/workflows/hand-written.yml";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A workflow `caller` with one job that calls `uses`.
fn caller(on: &str, uses: &str) -> String {
    format!(
        "[[workflow]]\nfile = \"caller\"\nname = \"Caller\"\n\n{on}\n\n[[workflow.job]]\nid = \"release\"\nuses = \"{uses}\"\npermissions = {{ contents = \"read\" }}\n"
    )
}

fn pinned(sha: &str) -> String {
    format!("tailrocks/velnor-new/.github/workflows/x.yml@{sha}")
}

#[test]
fn a_pinned_call_needs_exactly_forty_lower_case_hex_digits() -> TestResult {
    let accepted = rendered_file(&caller(PUSH, &pinned(SHA)), "caller")?;
    assert_eq!(
        job(&accepted, "release")?.get("uses").and_then(Node::text),
        Some(pinned(SHA).as_str())
    );
    let cases = [
        ("thirty-nine", SHA[..39].to_owned()),
        ("forty-one", format!("{SHA}0")),
        ("one", "a".to_owned()),
        ("non-hex", format!("{}g", &SHA[..39])),
        ("upper case", SHA.to_uppercase()),
        ("one upper case digit", format!("A{}", &SHA[1..])),
        ("empty", String::new()),
    ];
    for (what, sha) in cases {
        let message = refusal(&caller(PUSH, &pinned(&sha)))?;
        assert!(message.contains("full commit SHA"), "{what}: {message}");
    }
    Ok(())
}

#[test]
fn a_local_or_pinned_callee_is_refused_from_a_workflow_a_pull_request_or_merge_queue_starts()
-> TestResult {
    for on in [
        PULL_REQUEST,
        MERGE_GROUP,
        &format!("{PUSH}\n\n{PULL_REQUEST}"),
        &format!("{MERGE_GROUP}\n\n{PUSH}"),
    ] {
        for uses in [LOCAL.to_owned(), pinned(SHA)] {
            let message = refusal(&caller(on, &uses))?;
            assert!(
                message.contains("job release") && message.contains("fork fence"),
                "{on} {uses}: {message}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_local_or_pinned_callee_stays_allowed_from_push_dispatch_schedule_and_call() -> TestResult {
    let on = [
        PUSH,
        "[workflow.on.workflow_dispatch]",
        "[workflow.on]\nschedule = [\"23 4 * * 1\"]",
        "[workflow.on]\nworkflow_call = true",
        "[workflow.on.workflow_run]\nworkflows = [\"CI\"]",
    ];
    for on in on {
        for uses in [LOCAL.to_owned(), pinned(SHA)] {
            rendered_file(&caller(on, &uses), "caller")?;
        }
    }
    Ok(())
}

#[test]
fn a_declared_callee_stays_allowed_from_a_pull_request_workflow() -> TestResult {
    let config = CONFIG.replace(PUSH, PULL_REQUEST);
    let tree = rendered_file(&config, "caller")?;
    assert_eq!(
        job(&tree, "release")?.get("uses").and_then(Node::text),
        Some("./.github/workflows/leaf.yml")
    );
    let merge_queue = CONFIG.replace(PUSH, MERGE_GROUP);
    rendered_file(&merge_queue, "caller")?;
    Ok(())
}

#[test]
fn a_declared_callee_that_itself_calls_outside_is_refused_from_a_pull_request_workflow()
-> TestResult {
    let middle = |calls: &str| {
        format!(
            "[[workflow]]\nfile = \"middle\"\nname = \"Middle\"\n\n[[workflow.job]]\nid = \"inner\"\nuses = \"{calls}\"\n\n[[workflow]]\nfile = \"caller\"\nname = \"Caller\"\n\n{{ON}}\n\n[[workflow.job]]\nid = \"release\"\nuses = \"middle\"\n"
        )
    };
    for calls in [LOCAL.to_owned(), pinned(SHA)] {
        let template = middle(&calls);
        let from_pr = template.replace("{ON}", PULL_REQUEST);
        let message = refusal(&from_pr)?;
        assert!(
            message.contains("job release") && message.contains("fork fence"),
            "{calls}: {message}"
        );
        let from_push = template.replace("{ON}", PUSH);
        rendered_file(&from_push, "caller")?;
    }
    Ok(())
}

#[test]
fn a_ubuntu_label_without_a_version_is_not_github_hosted_and_gets_the_fork_fence() -> TestResult {
    for label in ["ubuntu-", "macos-", "windows-", "ubuntu-.", "ubuntu--arm"] {
        let tree = rendered(&workflow(
            &format!("runner = \"{label}\""),
            PULL_REQUEST,
            JOB,
        ))?;
        let runs_on = job(&tree, "check")?
            .get("runs-on")
            .and_then(Node::text)
            .ok_or("no runs-on")?;
        assert!(
            runs_on.contains("github.event.pull_request.head.repo.full_name"),
            "{label}: {runs_on}"
        );
        let message = refusal(&workflow(
            &format!("fork_runner = \"{label}\""),
            PULL_REQUEST,
            JOB,
        ))?;
        assert!(message.contains("GitHub-hosted"), "{label}: {message}");
    }
    for label in [
        "ubuntu-latest",
        "ubuntu-24.04",
        "ubuntu-24.04-arm",
        "macos-15",
        "windows-2025",
    ] {
        let tree = rendered(&workflow(
            &format!("runner = \"{label}\""),
            PULL_REQUEST,
            JOB,
        ))?;
        assert_eq!(
            job(&tree, "check")?.get("runs-on").and_then(Node::text),
            Some(label)
        );
    }
    Ok(())
}

/// A `workflow_run` workflow on a self-hosted runner.
fn follow_up(step: &str) -> String {
    workflow(
        "runner = \"velnor-linux\"",
        "[workflow.on.workflow_run]\nworkflows = [\"Rust CI\"]\nbranches = [\"main\"]",
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\nif = \"github.event.workflow_run.conclusion == 'success'\"\n\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n{step}\n"
        ),
    )
}

#[test]
fn a_workflow_run_on_a_self_hosted_runner_reads_github_event_only_in_an_if() -> TestResult {
    let tree = rendered(&follow_up(
        "env = { REPOSITORY = \"${{ github.repository }}\" }\nif = \"github.event.workflow_run.head_sha != ''\"",
    ))?;
    let check = job(&tree, "check")?;
    assert_eq!(
        check.get("if").and_then(Node::text),
        Some("${{ github.event.workflow_run.conclusion == 'success' }}")
    );
    for value in [
        "${{ github.event.workflow_run.head_sha }}",
        "${{ github.event.workflow_run.head_branch }}",
        "${{ github.head_ref }}",
        "${{ github.actor }}",
        "${{ github.event.workflow_run.head_commit.message }}",
    ] {
        let message = refusal(&follow_up(&format!("env = {{ X = \"{value}\" }}")))?;
        assert!(
            message.contains(value.trim_start_matches("${{ ").trim_end_matches(" }}")),
            "{value}: {message}"
        );
    }
    Ok(())
}
