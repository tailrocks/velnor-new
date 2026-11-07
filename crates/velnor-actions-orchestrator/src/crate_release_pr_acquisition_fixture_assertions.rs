// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{AcquisitionRequest, Download, Routes, acquire_preparation};
use base64::Engine as _;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub(super) fn assert_writer_calls(
    events: &Arc<Mutex<Vec<(String, Value)>>>,
    artifact: &crate::crate_release_pr_artifact::PreparationArtifact,
    root: &str,
    expected_tree: &str,
    sha: &str,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    let calls = events.lock().expect("events").clone();
    let writes: Vec<_> = calls
        .iter()
        .filter(|(route, _)| route.starts_with("POST "))
        .collect();
    assert_eq!(writes.len(), 5);
    assert!(
        calls
            .iter()
            .all(|(route, _)| route.starts_with("GET ") || route.starts_with("POST "))
    );
    let tree_write = writes
        .iter()
        .find(|(route, _)| route.ends_with("/git/trees"))
        .ok_or("tree write")?;
    assert_eq!(tree_write.1["base_tree"], root);
    assert_eq!(
        tree_write.1["tree"].as_array().ok_or("tree entries")?.len(),
        1
    );
    assert_eq!(tree_write.1["tree"][0]["path"], "Cargo.toml");
    assert_eq!(tree_write.1["tree"][0]["mode"], "100644");
    let expected_blob = crate::release_policy_admission::git_blob_sha1(&artifact.files[0].bytes)?;
    assert_eq!(tree_write.1["tree"][0]["sha"], expected_blob);
    let blob_write = writes
        .iter()
        .find(|(route, _)| route.ends_with("/git/blobs"))
        .ok_or("blob write")?;
    assert_eq!(blob_write.1["encoding"], "base64");
    let sent_bytes = base64::engine::general_purpose::STANDARD
        .decode(blob_write.1["content"].as_str().ok_or("blob content")?)?;
    assert_eq!(sent_bytes, artifact.files[0].bytes);
    let commit_write = writes
        .iter()
        .find(|(route, _)| route.ends_with("/git/commits"))
        .ok_or("commit write")?;
    assert_eq!(commit_write.1["tree"], expected_tree);
    assert_eq!(commit_write.1["parents"], serde_json::json!([sha]));
    let branch_write = writes
        .iter()
        .find(|(route, _)| route.ends_with("/git/refs"))
        .ok_or("ref write")?;
    assert_eq!(branch_write.1["ref"], "refs/heads/release-plz-42-1");
    assert!(branch_write.1.get("force").is_none());
    let pull_write = writes
        .iter()
        .find(|(route, _)| route.ends_with("/pulls"))
        .ok_or("pull write")?;
    assert_eq!(pull_write.1["head"], "release-plz-42-1");
    assert_eq!(pull_write.1["base"], intent.default_branch);
    Ok(())
}

pub(super) fn assert_rejections(
    request: &AcquisitionRequest<'_>,
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
    download: &Download,
    routes: &Routes,
    prefix: &str,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) {
    let bad = AcquisitionRequest {
        content_digest: &"0".repeat(64),
        ..*request
    };
    assert!(acquire_preparation(&bad, github, download).is_err());
    let bad = AcquisitionRequest {
        archive_digest: &"0".repeat(64),
        ..*request
    };
    assert!(acquire_preparation(&bad, github, download).is_err());
    let mut corrupt = download.bytes.clone();
    corrupt[0] ^= 1;
    assert!(
        acquire_preparation(
            request,
            github,
            &Download {
                bytes: corrupt,
                move_ref: None
            }
        )
        .is_err()
    );
    let jobs_path = format!("{prefix}/actions/runs/42/attempts/1/jobs?per_page=100");
    let old_jobs = routes.lock().expect("routes")[&jobs_path].clone();
    routes
        .lock()
        .expect("routes")
        .get_mut(&jobs_path)
        .expect("jobs")["jobs"][0]["conclusion"] = serde_json::json!("failure");
    assert!(acquire_preparation(request, github, download).is_err());
    routes.lock().expect("routes").insert(jobs_path, old_jobs);
    let ref_path = format!("{prefix}/git/ref/heads/{}", intent.default_branch);
    let changed_ref = Download {
        bytes: download.bytes.clone(),
        move_ref: Some((Arc::clone(routes), ref_path)),
    };
    assert!(acquire_preparation(request, github, &changed_ref).is_err());
}
