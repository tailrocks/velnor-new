// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::continuation::{verify_real_journal_continuation, verify_test_previous_continuation};
use super::responses::{
    TestJournalState, test_changing_workflow_response, test_github_admission_response,
    test_journal_response, test_metadata_responses,
};

#[cfg(unix)]
pub(super) fn run_test_transport_child() -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::CrateReleaseRepairWrite as _;
    use crate::crate_release_github::GitHubAdmissionRead as _;
    use crate::crate_release_journal::JournalIo as _;
    use crate::crate_release_registry::CrateRegistryRead as _;

    assert!(matches!(
        super::super::run_crate_release("journal-start"),
        Err(crate::OrchestratorError::Internal { problem })
            if problem == "crate_release_authority_not_qualified"
    ));
    assert!(matches!(
        super::super::run_qualified_crate_release("journal-start"),
        Err(crate::OrchestratorError::Internal { problem })
            if problem == "crate_release_missing_GITHUB_WORKSPACE"
    ));
    let github = crate::crate_release_github::NativeGitHubAdmissionRead::new("fixture".into())?;
    assert_eq!(github.get("repos/owner/project")?["ok"], true);
    let registry = crate::crate_release_registry::NativeCrateRegistryRead;
    assert_eq!(registry.get("/api/v1/crates/example")?.status, 200);
    let sha = "b".repeat(40);
    github.create_tag("owner/project", "demo-v0.1.1", &sha)?;
    assert_eq!(
        github
            .get_optional("repos/owner/project/git/ref/tags/demo-v0.1.1")?
            .ok_or("missing tag")?["object"]["sha"],
        sha
    );
    github.create_release("owner/project", "demo-v0.1.1", &sha, "0.1.1")?;
    assert_eq!(
        github
            .get_optional("repos/owner/project/releases/tags/demo-v0.1.1")?
            .ok_or("missing release")?["id"],
        12
    );
    let journal = crate::crate_release_journal_adapter::JournalAdapter::new(
        crate::crate_release_github::NativeGitHubAdmissionRead::new("fixture".into())?,
    );
    let blob = journal.create_blob("owner/project", b"fixture receipt")?;
    assert_eq!(
        journal.get_blob("owner/project", &blob)?,
        b"fixture receipt"
    );
    let tag = crate::crate_release_journal::JournalTag {
        name: "velnor-receipt/12/1/operation-1".into(),
        object_type: "blob".into(),
        object_sha: blob,
        message: "velnor-release-receipt-v1".into(),
    };
    let tag_sha = journal.create_tag_object("owner/project", &tag)?;
    let reference = format!("refs/tags/{}", tag.name);
    journal.create_ref("owner/project", &reference, &tag_sha)?;
    assert_eq!(
        journal.get_ref("owner/project", &reference)?,
        Some(tag_sha.clone())
    );
    assert!(
        journal
            .create_ref("owner/project", &reference, &tag_sha)
            .is_err()
    );
    assert_eq!(journal.get_ref("owner/project", &reference)?, Some(tag_sha));
    let (owners, presence) =
        crate::crate_release_registry::inspect_crate_registry(&registry, "example", "0.1.1")?;
    assert!(
        matches!(owners, crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } if owners.len() == 1)
    );
    assert_eq!(
        presence,
        crate::crate_release_registry::CrateRegistryPresence::Absent
    );
    verify_test_admissions(&github)?;
    verify_required_workflow_selection(&github)?;
    verify_test_previous_continuation(&github)?;
    verify_test_journal_refs(&journal)?;
    verify_real_journal_continuation(&journal)?;
    #[cfg(target_os = "linux")]
    super::continuation::verify_real_direct_history(&github)?;
    Ok(())
}

#[cfg(unix)]
fn verify_required_workflow_selection(
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        crate::crate_release_github::verify_required_check(
            github,
            "owner/project",
            &"e".repeat(40),
            ".github/workflows/ci.yml"
        )?,
        31
    );
    assert!(matches!(
        crate::crate_release_github::verify_required_check(
            github,
            "owner/project",
            &"f".repeat(40),
            ".github/workflows/ci.yml"
        ),
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_required_failed"
    ));
    assert!(matches!(
        crate::crate_release_github::verify_required_check(
            github,
            "owner/project",
            &"d".repeat(40),
            ".github/workflows/ci.yml"
        ),
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_runs_changed"
    ));
    let pending = crate::crate_release_github::verify_required_check(
        github,
        "owner/project",
        &"6".repeat(40),
        ".github/workflows/ci.yml",
    );
    assert!(matches!(pending,
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_required_pending"
    ));
    let changed_during_origin_check = crate::crate_release_github::verify_required_check(
        github,
        "owner/project",
        &"0".repeat(40),
        ".github/workflows/ci.yml",
    );
    assert!(matches!(changed_during_origin_check,
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_runs_changed"
    ));
    let rerun_pending = crate::crate_release_github::verify_required_check(
        github,
        "owner/project",
        &"2".repeat(40),
        ".github/workflows/ci.yml",
    );
    assert!(matches!(rerun_pending,
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_required_pending"
    ));
    let rerun_missing_check = crate::crate_release_github::verify_required_check(
        github,
        "owner/project",
        &"c".repeat(40),
        ".github/workflows/ci.yml",
    );
    assert!(matches!(rerun_missing_check,
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_required_pending"
    ));
    assert_eq!(
        crate::crate_release_github::verify_required_check(
            github,
            "owner/project",
            &"1".repeat(40),
            ".github/workflows/ci.yml",
        )?,
        51
    );
    verify_required_workflow_retries(github)?;
    Ok(())
}

#[cfg(unix)]
fn verify_required_workflow_retries(
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
) -> Result<(), Box<dyn std::error::Error>> {
    let waits = std::cell::Cell::new(0);
    let recovered = super::super::poll_pending_admission(
        || {
            crate::crate_release_github::verify_required_check(
                github,
                "owner/project",
                &"5".repeat(40),
                ".github/workflows/ci.yml",
            )
        },
        || true,
        || waits.set(waits.get() + 1),
    )?;
    assert_eq!(recovered, 33);
    assert_eq!(waits.get(), 1);
    for (source_digit, expected_id) in [("4", 41), ("3", 42)] {
        let waits = std::cell::Cell::new(0);
        let recovered = super::super::poll_pending_admission(
            || {
                crate::crate_release_github::verify_required_check(
                    github,
                    "owner/project",
                    &source_digit.repeat(40),
                    ".github/workflows/ci.yml",
                )
            },
            || true,
            || waits.set(waits.get() + 1),
        )?;
        assert_eq!(recovered, expected_id);
        assert_eq!(waits.get(), 1);
    }
    let expired = super::super::poll_pending_admission(
        || {
            crate::crate_release_github::verify_required_check(
                github,
                "owner/project",
                &"8".repeat(40),
                ".github/workflows/ci.yml",
            )
        },
        || false,
        || panic!("expired deadline was retried"),
    );
    assert!(matches!(expired,
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_runs_changed"
    ));
    let deadline_expired = std::cell::Cell::new(false);
    let expired_during_wait = super::super::poll_pending_admission(
        || {
            crate::crate_release_github::verify_required_check(
                github,
                "owner/project",
                &"7".repeat(40),
                ".github/workflows/ci.yml",
            )
        },
        || !deadline_expired.get(),
        || deadline_expired.set(true),
    );
    assert!(matches!(expired_during_wait,
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_runs_changed"
    ));
    let invalid = super::super::poll_pending_admission(
        || {
            crate::crate_release_github::verify_required_check(
                github,
                "owner/project",
                &"f".repeat(40),
                ".github/workflows/ci.yml",
            )
        },
        || true,
        || panic!("failed required check was retried"),
    );
    assert!(matches!(invalid,
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_required_failed"
    ));
    Ok(())
}

#[cfg(unix)]
fn verify_test_journal_refs(
    journal: &crate::crate_release_journal_adapter::JournalAdapter<
        crate::crate_release_github::NativeGitHubAdmissionRead,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{JournalIo as _, JournalOperationRefs as _};
    for name in [
        "velnor-receipt/12/1/publication-started",
        "velnor-receipt/12/1/reconciled",
    ] {
        let tag = crate::crate_release_journal::JournalTag {
            name: name.into(),
            object_type: "blob".into(),
            object_sha: journal.create_blob("owner/project", name.as_bytes())?,
            message: "velnor-release-receipt-v1".into(),
        };
        let sha = journal.create_tag_object("owner/project", &tag)?;
        let reference = format!("refs/tags/{name}");
        assert_eq!(journal.get_ref("owner/project", &reference)?, None);
        journal.create_ref("owner/project", &reference, &sha)?;
        assert_eq!(journal.get_ref("owner/project", &reference)?, Some(sha));
    }
    let identity = crate::crate_release_journal::JournalIdentity {
        repository: "owner/project".into(),
        workflow_sha: "b".repeat(40),
        source_sha: "a".repeat(40),
        run_id: 12,
        run_attempt: 1,
    };
    assert_eq!(
        journal
            .list_operation_refs(&identity)?
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["refs/tags/velnor-receipt/12/1/operation-1"]
    );
    Ok(())
}

#[cfg(unix)]
fn verify_test_admissions(
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = crate::crate_release_github::GitHubAdmissionRequest {
        repository: "owner/project".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        trusted_reviewer_id: 139_017,
        workflow_path: ".github/workflows/release.yml".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        workflow_sha: "b".repeat(40),
        run_id: 12,
        run_attempt: 1,
        event: "workflow_dispatch".into(),
        source_sha: "a".repeat(40),
        recovery_sha: Some("a".repeat(40)),
    };
    let candidate = crate::crate_release_github::verify_github_evidence(&request, github)?;
    crate::crate_release_github::authorize_github_candidate(&request, &candidate, github)?;
    let mut other_source = request.clone();
    other_source.source_sha = "e".repeat(40);
    other_source.recovery_sha = Some(other_source.source_sha.clone());
    let selected = crate::crate_release_github::verify_github_evidence(&other_source, github)?;
    assert_eq!(selected.required_check_id, 31);
    other_source.source_sha = "9".repeat(40);
    other_source.recovery_sha = Some(other_source.source_sha.clone());
    assert!(matches!(
        crate::crate_release_github::verify_github_evidence(&other_source, github),
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_required_pending"
    ));
    let normal = crate::crate_release_github::GitHubAdmissionRequest {
        run_id: 13,
        event: "push".into(),
        source_sha: request.workflow_sha.clone(),
        recovery_sha: None,
        ..request
    };
    let candidate = crate::crate_release_github::verify_github_evidence(&normal, github)?;
    crate::crate_release_github::authorize_github_candidate(&normal, &candidate, github)?;
    Ok(())
}

#[cfg(unix)]
pub(super) fn serve_test_transport(
    listener: &std::net::TcpListener,
    done: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    use std::io::Write as _;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let mut paths = Vec::new();
    let mut state = TestJournalState::default();
    while !done.load(std::sync::atomic::Ordering::Acquire) && std::time::Instant::now() < deadline {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let (line, body) = read_test_request(&mut stream)?;
        let (status, response) = test_service_response(&line, &body, &paths, &mut state)?;
        paths.push(line);
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).map_err(|e| e.to_string())?;
    }
    if paths.is_empty() {
        return Err("no fixture requests".into());
    }
    Ok(())
}

#[cfg(unix)]
pub(in super::super) fn read_test_request(
    stream: &mut std::net::TcpStream,
) -> Result<(String, Vec<u8>), String> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let size = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if size == 0 || bytes.len() + size > 65_536 {
            return Err("invalid fixture request".into());
        }
        bytes.extend_from_slice(&buffer[..size]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let header = std::str::from_utf8(&bytes[..end]).map_err(|e| e.to_string())?;
            let length = header
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>())
                })
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return Ok((
                    header.lines().next().unwrap_or_default().to_owned(),
                    bytes[end + 4..end + 4 + length].to_vec(),
                ));
            }
        }
    }
}

#[cfg(unix)]
fn test_service_response(
    line: &str,
    body: &[u8],
    previous: &[String],
    state: &mut TestJournalState,
) -> Result<(&'static str, String), String> {
    if line.contains("/git/blobs") {
        return test_blob_response(line, body, state);
    }
    let sha = "b".repeat(40);
    let (tag, release) = test_metadata_responses(&sha);
    let (status, value) = match line {
        "GET /repos/owner/project HTTP/1.1" => (
            "200 OK",
            serde_json::json!({"ok":true,"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"},"default_branch":"main"}),
        ),
        "GET /repos/owner/project/rulesets?targets=tag&includes_parents=true&per_page=100 HTTP/1.1" => {
            ("200 OK", serde_json::json!([{"id":1}]))
        }
        "GET /repos/owner/project/rulesets/1 HTTP/1.1" => (
            "200 OK",
            serde_json::json!({"id":1,"target":"tag","enforcement":"active","bypass_actors":[],"conditions":{"ref_name":{"include":["~ALL"],"exclude":[]}},"rules":[{"type":"update","parameters":{"update_allows_fetch_and_merge":false}},{"type":"deletion"}]}),
        ),
        "GET /api/v1/crates/example HTTP/1.1" => (
            "200 OK",
            serde_json::json!({"crate":{"id":"example","name":"example","versions":[1]},"versions":[{"id":1,"crate":"example","num":"0.1.0","yanked":false}]}),
        ),
        "GET /api/v1/crates/example/owners HTTP/1.1" => (
            "200 OK",
            serde_json::json!({"users":[{"id":370_532,"login":"owner","kind":"user"}]}),
        ),
        "GET /api/v1/crates/example/0.1.1 HTTP/1.1" => {
            ("404 Not Found", serde_json::json!({"errors":[]}))
        }
        "POST /repos/owner/project/git/refs HTTP/1.1" if previous.len() < 7 => {
            let value: serde_json::Value =
                serde_json::from_slice(body).map_err(|e| e.to_string())?;
            if value != serde_json::json!({"ref":"refs/tags/demo-v0.1.1","sha":sha}) {
                return Err("wrong tag mutation".into());
            }
            ("201 Created", tag)
        }
        "GET /repos/owner/project/git/ref/tags/demo-v0.1.1 HTTP/1.1"
            if previous
                .iter()
                .any(|entry| entry.starts_with("POST /repos/owner/project/git/refs ")) =>
        {
            ("200 OK", tag)
        }
        "POST /repos/owner/project/releases HTTP/1.1"
            if previous
                .iter()
                .any(|entry| entry.starts_with("GET /repos/owner/project/git/ref/tags/")) =>
        {
            let value: serde_json::Value =
                serde_json::from_slice(body).map_err(|e| e.to_string())?;
            if value
                != serde_json::json!({"tag_name":"demo-v0.1.1","name":"demo-v0.1.1","target_commitish":sha,"make_latest":"false","prerelease":false})
            {
                return Err("wrong release mutation".into());
            }
            ("201 Created", release)
        }
        "GET /repos/owner/project/releases/tags/demo-v0.1.1 HTTP/1.1"
            if previous
                .iter()
                .any(|entry| entry.starts_with("POST /repos/owner/project/releases ")) =>
        {
            ("200 OK", release)
        }
        _ => {
            if let Some(value) = test_rerun_response(line)
                .or_else(|| test_pagination_change_response(line, previous))
                .or_else(|| test_changing_workflow_response(line, previous))
                .or_else(|| {
                    let (policy, source) = selected_history(previous);
                    test_github_admission_response(line, policy, source)
                })
            {
                ("200 OK", value)
            } else {
                return test_journal_response(line, body, state);
            }
        }
    };
    Ok((status, value.to_string()))
}

#[cfg(unix)]
fn test_rerun_response(line: &str) -> Option<serde_json::Value> {
    let success_sha = "1".repeat(40);
    let pending_sha = "2".repeat(40);
    let missing_sha = "c".repeat(40);
    let missing =
        line.contains(&missing_sha) || line.contains("/actions/runs/1153/attempts/2/jobs?");
    let completed = missing
        || line.contains(&success_sha)
        || line.contains("/actions/runs/1151/attempts/2/jobs?");
    let pending = line.contains(&pending_sha);
    if !completed && !pending {
        return None;
    }
    let (sha, suite, id) = if missing {
        (&missing_sha, 153, 1153)
    } else if completed {
        (&success_sha, 151, 1151)
    } else {
        (&pending_sha, 152, 1152)
    };
    if line.contains("/actions/runs?") {
        return Some(
            serde_json::json!({"total_count":1,"workflow_runs":[{"id":id,"run_number":suite,"run_attempt":2,"check_suite_id":suite,"head_sha":sha,"path":".github/workflows/ci.yml","status":if completed {"completed"} else {"in_progress"},"conclusion":if completed {Some("success")} else {None}}]}),
        );
    }
    if line.contains("/attempts/2/jobs?") {
        return Some(
            serde_json::json!({"total_count":1,"jobs":[{"id":51,"name":"Required","run_id":id,"run_attempt":2,"status":"completed","conclusion":"success","check_run_url":"https://api.github.com/repos/owner/project/check-runs/51"}]}),
        );
    }
    if line.contains("/check-runs?") {
        let mut checks = vec![
            serde_json::json!({"id":50,"name":"Required","head_sha":sha,"check_suite":{"id":suite},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"}),
        ];
        if completed && !missing {
            checks.push(serde_json::json!({"id":51,"name":"Required","head_sha":sha,"check_suite":{"id":suite},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:41:00Z"}));
        }
        return Some(serde_json::json!({"total_count":checks.len(),"check_runs":checks}));
    }
    None
}

#[cfg(unix)]
fn test_pagination_change_response(line: &str, previous: &[String]) -> Option<serde_json::Value> {
    let sha_checks = "4".repeat(40);
    let sha_jobs = "3".repeat(40);
    let is_checks =
        line.contains(&sha_checks) || line.contains("/actions/runs/1144/attempts/1/jobs?");
    let is_jobs = line.contains(&sha_jobs) || line.contains("/actions/runs/1143/attempts/1/jobs?");
    if !is_checks && !is_jobs {
        return None;
    }
    let (sha, suite, run_id, check_id) = if is_checks {
        (&sha_checks, 144, 1144, 41)
    } else {
        (&sha_jobs, 143, 1143, 42)
    };
    if line.contains("/actions/runs?") {
        return Some(
            serde_json::json!({"total_count":1,"workflow_runs":[{"id":run_id,"run_number":suite,"run_attempt":1,"check_suite_id":suite,"head_sha":sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]}),
        );
    }
    if line.contains("/attempts/1/jobs?") {
        let required = serde_json::json!({"id":check_id,"name":"Required","run_id":run_id,"run_attempt":1,"status":"completed","conclusion":"success","check_run_url":format!("https://api.github.com/repos/owner/project/check-runs/{check_id}")});
        if is_checks {
            return Some(serde_json::json!({"total_count":1,"jobs":[required]}));
        }
        let second = line.contains("page=2");
        let prior_second = previous.iter().any(|entry| {
            entry.contains("/actions/runs/1143/attempts/1/jobs?") && entry.contains("page=2")
        });
        let mut jobs = vec![];
        if second {
            jobs.push(serde_json::json!({"id":9000,"name":"Other"}));
        } else {
            jobs.push(required);
            for id in 1000..1099 {
                jobs.push(serde_json::json!({"id":id,"name":"Other"}));
            }
        }
        return Some(
            serde_json::json!({"total_count":if second && !prior_second {102} else {101},"jobs":jobs}),
        );
    }
    if line.contains("/check-runs?") {
        let required = serde_json::json!({"id":check_id,"name":"Required","head_sha":sha,"check_suite":{"id":suite},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"});
        if is_jobs {
            return Some(serde_json::json!({"total_count":1,"check_runs":[required]}));
        }
        let second = line.contains("page=2");
        let prior_second = previous.iter().any(|entry| {
            entry.contains(&sha_checks)
                && entry.contains("/check-runs?")
                && entry.contains("page=2")
        });
        let mut checks = vec![];
        if second {
            checks.push(serde_json::json!({"id":9000,"name":"Other"}));
        } else {
            checks.push(required);
            for id in 1000..1099 {
                checks.push(serde_json::json!({"id":id,"name":"Other"}));
            }
        }
        return Some(
            serde_json::json!({"total_count":if second && !prior_second {102} else {101},"check_runs":checks}),
        );
    }
    None
}

#[cfg(unix)]
fn selected_history(previous: &[String]) -> (&str, &str) {
    previous
        .iter()
        .rev()
        .find_map(|entry| {
            let values = entry
                .strip_prefix("GET /repos/owner/project/compare/")?
                .strip_suffix(" HTTP/1.1")?;
            let (source, policy) = values.split_once("...")?;
            (policy.len() == 40
                && source.len() == 40
                && source.as_bytes().windows(2).any(|pair| pair[0] != pair[1]))
            .then_some((policy, source))
        })
        .unwrap_or((
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ))
}

#[cfg(unix)]
fn test_blob_response(
    line: &str,
    body: &[u8],
    state: &mut TestJournalState,
) -> Result<(&'static str, String), String> {
    use base64::Engine as _;
    let (status, value) = match line {
        "POST /repos/owner/project/git/blobs HTTP/1.1" => {
            let value: serde_json::Value =
                serde_json::from_slice(body).map_err(|e| e.to_string())?;
            if value["encoding"] != "base64" {
                return Err("wrong blob encoding".into());
            }
            let content = base64::engine::general_purpose::STANDARD
                .decode(value["content"].as_str().ok_or("missing blob")?)
                .map_err(|e| e.to_string())?;
            let sha = crate::release_policy_admission::git_blob_sha1(&content)
                .map_err(|e| e.to_string())?;
            state.blobs.insert(sha.clone(), content);
            ("201 Created", serde_json::json!({"sha":sha}))
        }
        path if path.starts_with("GET /repos/owner/project/git/blobs/") => {
            let sha = path
                .strip_prefix("GET /repos/owner/project/git/blobs/")
                .and_then(|value| value.strip_suffix(" HTTP/1.1"))
                .ok_or("invalid blob path")?;
            let content = state.blobs.get(sha).ok_or("missing blob")?;
            (
                "200 OK",
                serde_json::json!({"sha":sha,"encoding":"base64","size":content.len(),"content":base64::engine::general_purpose::STANDARD.encode(content)}),
            )
        }
        _ => return Err(format!("unexpected blob request: {line}")),
    };
    Ok((status, value.to_string()))
}
