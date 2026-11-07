// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::continuation::{verify_real_journal_continuation, verify_test_previous_continuation};
use super::responses::{
    TestJournalState, test_github_admission_response, test_journal_response,
    test_metadata_responses,
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
    verify_test_previous_continuation(&github)?;
    verify_test_journal_refs(&journal)?;
    verify_real_journal_continuation(&journal)?;
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
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
            if let Some(value) =
                test_github_admission_response(line, &"b".repeat(40), &"a".repeat(40))
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
