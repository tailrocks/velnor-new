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

#[path = "crate_release_runtime_fixture_transport_handlers.rs"]
mod handlers;
pub(in super::super) use handlers::read_test_request;
use handlers::*;
