// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::fixture::Candidate;
use super::registry::TOKEN;

#[test]
fn the_command_refuses_a_pull_request_ref_before_it_runs_gh_or_reaches_the_network() {
    let candidate = Candidate::new("cli-pull-request");
    let mut vars = candidate.env();
    vars.insert("GITHUB_REF".to_owned(), "refs/pull/7/merge".to_owned());
    vars.insert("GITHUB_EVENT_NAME".to_owned(), "pull_request".to_owned());
    let (ok, stdout, stderr) = candidate.binary(&vars, &[]);
    assert!(!ok, "{stdout}");
    assert!(
        stderr.contains("Publication is only for workflow_dispatch on refs/heads/main."),
        "{stderr}"
    );
    assert_eq!(candidate.gh_log(), "");
    assert!(!candidate.path("evidence").exists());
}

#[test]
fn the_command_stops_at_a_failed_attestation_found_through_the_gh_on_path() {
    let candidate = Candidate::new("cli-attestation");
    candidate.fail_attestation();
    let (ok, stdout, stderr) = candidate.binary(&candidate.env(), &[]);
    assert!(!ok, "{stdout}");
    assert!(stderr.contains("GitHub CLI request failed."), "{stderr}");
    assert!(!stderr.contains(TOKEN) && !stdout.contains(TOKEN));
    let calls = candidate.gh_log();
    assert!(calls.contains("attestation verify"), "{calls}");
    assert!(!candidate.path("evidence").exists());
}

#[test]
fn the_publishing_workflow_and_event_are_flags_and_a_pull_request_event_cannot_be_chosen() {
    let candidate = Candidate::new("cli-flags");
    let mut vars = candidate.env();
    vars.insert("GITHUB_EVENT_NAME".to_owned(), "push".to_owned());
    let release = "tailrocks/velnor-new/.github/workflows/release.yml";
    vars.insert(
        "GITHUB_WORKFLOW_REF".to_owned(),
        format!("{release}@refs/heads/main"),
    );
    let mut other = candidate.record.clone();
    other["workflow"]["run_id"] = serde_json::json!(78);
    std::fs::write(candidate.path("out/candidate.json"), other.to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let flags = ["--workflow", release, "--event", "push"];
    let (ok, _, stderr) = candidate.binary(&vars, &flags);
    assert!(!ok);
    assert!(
        stderr.contains("Candidate run identity is different from the producer run identity."),
        "the gate accepted the flags and the record check ran: {stderr}"
    );
    assert!(
        candidate
            .gh_log()
            .contains("api repos/tailrocks/velnor-new")
    );
    let refused = ["--event", "pull_request"];
    let (ok, _, stderr) = candidate.binary(&candidate.env(), &refused);
    assert!(!ok);
    assert!(
        stderr.contains("Publishing event must be workflow_dispatch or push."),
        "{stderr}"
    );
}
