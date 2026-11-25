// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_ci::image_publish::checks::ci_path;
use velnor_ci::io::{Run, Transport, publish_with};
use zeroize::Zeroizing;

use super::fixture::{ARTIFACTS, Candidate};
use super::publish::{PASS, refused};
use super::registry::{Fake, Flaws, TOKEN};

pub(super) fn gate(label: &str, change: impl Fn(&Candidate), message: &str) {
    let candidate = Candidate::new(label);
    change(&candidate);
    let fake = Fake::default();
    assert_eq!(refused(&candidate, &fake), message, "{label}");
    assert!(fake.seen().is_empty(), "{label}");
    assert!(!candidate.path("evidence").exists(), "{label}");
}

#[test]
fn a_moved_main_tip_leaves_the_registry_untouched() {
    gate(
        "tip",
        |c| {
            c.api(
                "repos/tailrocks/velnor-new/commits/main",
                &json!({"sha": "2".repeat(40)}),
            );
        },
        "Source SHA is not the current main commit.",
    );
}

#[test]
fn a_foreign_repository_id_leaves_the_registry_untouched() {
    gate(
        "repository",
        |c| {
            let wrong = json!({"full_name": "tailrocks/velnor-new", "id": 5});
            c.api("repos/tailrocks/velnor-new", &wrong);
        },
        "GitHub repository ID is not the same as the canonical repository ID.",
    );
}

#[test]
fn a_run_without_its_candidate_artifact_leaves_the_registry_untouched() {
    gate(
        "artifact",
        |c| {
            c.api(ARTIFACTS, &json!({"artifacts": []}));
        },
        "This workflow run must have one candidate artifact.",
    );
}

#[test]
fn a_failed_ci_run_of_the_commit_leaves_the_registry_untouched() {
    gate(
        "ci-failed",
        |c| {
            let run = json!({"id": 50, "run_number": 5, "run_attempt": 1,
                "status": "completed", "conclusion": "failure",
                "path": ".github/workflows/ci.yml", "event": "push",
                "head_branch": "main", "head_sha": c.head,
                "head_repository": {"full_name": "tailrocks/velnor-new"}});
            c.api(&ci_path(&c.head), &json!({"workflow_runs": [run]}));
        },
        "The latest canonical main CI run has no `success` conclusion.",
    );
}

#[test]
fn a_record_of_another_run_leaves_the_registry_untouched() {
    gate(
        "other-run",
        |c| {
            let mut other = c.record.clone();
            other["workflow"]["run_id"] = json!(78);
            fs::write(c.path("out/candidate.json"), other.to_string())
                .unwrap_or_else(|e| unreachable!("{e}"));
        },
        "Candidate run identity is different from the producer run identity.",
    );
}

#[test]
fn an_empty_ci_listing_is_retried_on_the_schedule_and_then_refused() {
    let candidate = Candidate::new("ci-empty");
    candidate.api(&ci_path(&candidate.head), &json!({"workflow_runs": []}));
    let fake = Fake::default();
    let (result, pauses) = candidate.publish(&fake, candidate.env());
    let seconds: Vec<u64> = pauses.iter().map(Duration::as_secs).collect();
    assert_eq!(seconds, [3, 6, 12, 24, 45]);
    assert_eq!(
        result.err().as_deref(),
        Some("No canonical main CI push run exists for this source.")
    );
    assert!(fake.seen().is_empty());
}

#[test]
fn a_late_ci_run_is_found_by_the_retry() {
    let candidate = Candidate::new("ci-late");
    let path = ci_path(&candidate.head);
    let name = path.replace(['/', '?', '&', '='], "_");
    let listing = fs::read_to_string(candidate.path("api").join(name)).unwrap_or_default();
    let ready: Value = serde_json::from_str(&listing).unwrap_or_default();
    candidate.api_call(&path, 1, &json!({"workflow_runs": []}));
    candidate.api_call(&path, 2, &ready);
    let (result, pauses) = candidate.publish(&Fake::default(), candidate.env());
    assert_eq!(result.as_deref(), Ok(PASS));
    assert_eq!(pauses.len(), 1);
}

#[test]
fn publishing_again_uploads_nothing_and_still_checks_every_object() {
    let candidate = Candidate::new("again");
    let first = Fake::default();
    let published = candidate.publish(&first, candidate.env());
    assert_eq!(published.0.as_deref(), Ok(PASS));
    fs::remove_dir_all(candidate.path("evidence")).unwrap_or_else(|e| unreachable!("{e}"));
    let second = Fake::default();
    second.holds(&first);
    let again = candidate.publish(&second, candidate.env());
    assert_eq!(again.0.as_deref(), Ok(PASS));
    let seen = second.seen();
    let writes: Vec<_> = seen
        .iter()
        .filter(|seen| seen.method == "POST" || seen.method == "PUT")
        .collect();
    assert!(writes.is_empty(), "{writes:?}");
    let gets = seen.iter().filter(|seen| seen.method == "GET").count();
    assert!(gets >= 5, "the objects were read back: {gets}");
}

#[test]
fn a_blob_upload_whose_response_is_lost_is_accepted_when_the_registry_has_it() {
    let candidate = Candidate::new("lost-response");
    let fake = Fake::with(Flaws {
        drop_blob_put: true,
        ..Flaws::default()
    });
    let published = candidate.publish(&fake, candidate.env());
    assert_eq!(published.0.as_deref(), Ok(PASS));
}

#[test]
fn a_registry_that_serves_other_bytes_fails_the_final_check_and_writes_no_evidence() {
    let candidate = Candidate::new("corrupt");
    let fake = Fake::with(Flaws {
        corrupt_blob: true,
        ..Flaws::default()
    });
    assert_eq!(
        refused(&candidate, &fake),
        "Registry response bytes have an incorrect digest or size."
    );
    assert!(!candidate.path("evidence/publication.json").exists());
}

#[test]
fn a_blob_redirect_outside_the_download_host_is_refused() {
    let candidate = Candidate::new("redirect");
    for target in [
        "https://evil.example/ghcr1/blobs/{digest}",
        "http://pkg-containers.githubusercontent.com/ghcr1/blobs/{digest}",
        "https://ghcr.io/v2/tailrocks/velnor-job/blobs/{digest}",
    ] {
        let fake = Fake::with(Flaws {
            redirect: Some(target.to_owned()),
            ..Flaws::default()
        });
        let message = refused(&candidate, &fake);
        assert!(
            message.starts_with("Registry download URL"),
            "{target}: {message}"
        );
        let seen = fake.seen();
        assert!(seen.iter().all(|seen| !seen.url.contains("evil.example")));
        let _ = fs::remove_dir_all(candidate.path("evidence"));
    }
}

#[test]
fn the_result_must_be_a_new_file() {
    let candidate = Candidate::new("result-path");
    let fake = Fake::default();
    let gh = candidate.path("bin/gh");
    let pause = |_: Duration| {};
    let run = |result: &str| {
        let mut args = candidate.args();
        args.result = candidate.path(result);
        publish_with(
            &args,
            &Run {
                vars: candidate.env(),
                token: Zeroizing::new(TOKEN.to_owned()),
                gh: &gh,
                transport: &fake as &dyn Transport,
                pause: &pause,
            },
        )
        .map_err(|e| e.to_string())
    };
    assert_eq!(
        run("out/candidate.json").err().as_deref(),
        Some("Result path must be new.")
    );
    assert!(fake.seen().is_empty());
}
