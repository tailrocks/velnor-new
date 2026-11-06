// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "The complete verified input is visible in one fixture"
)]
fn derives_reviewable_plan_from_verified_source_tree() -> Result<(), OrchestratorError> {
    use crate::crate_release_pr_tree::{GitHubTrees, TreeItem, git_tree_sha};
    struct Api {
        sha: String,
        blob: String,
    }
    impl crate::crate_release_github::GitHubAdmissionRead for Api {
        fn get(&self, route: &str) -> Result<serde_json::Value, OrchestratorError> {
            assert_eq!(route, format!("repos/owner/project/git/trees/{}", self.sha));
            Ok(
                serde_json::json!({"sha":self.sha,"truncated":false,"tree":[{"path":"Cargo.toml","mode":"100755","type":"blob","sha":self.blob}]}),
            )
        }
        fn get_optional(&self, _: &str) -> Result<Option<serde_json::Value>, OrchestratorError> {
            Err(internal("unexpected_optional"))
        }
    }
    let old_blob = crate::release_policy_admission::git_blob_sha1(b"old")?;
    let root = git_tree_sha(&[TreeItem {
        name: "Cargo.toml".into(),
        mode: "100755".into(),
        sha: old_blob.clone(),
    }])?;
    let api = Api {
        sha: root.clone(),
        blob: old_blob,
    };
    let trees = GitHubTrees {
        api: &api,
        repository: "owner/project",
    };
    let source = "a".repeat(40);
    let intent = velnor_actions_contract::config::CrateReleaseIntent {
        schema: 1,
        repository: "owner/project".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        trusted_reviewer_id: 1,
        rust_version: "1.90.0".into(),
        generator_version: "0.1.73".into(),
        generator_source_sha: "b".repeat(40),
        generator_manifest_sha256: "c".repeat(64),
        release_plz_version: "0.4.3".into(),
        runner: "ubuntu-26.04".into(),
        authentication: velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing,
        manifest_path: "Cargo.toml".into(),
        pr_write_paths: vec!["Cargo.toml".into()],
        registry: "crates-io".into(),
        packages: vec!["sample".into()],
        publish_features: BTreeMap::new(),
        version_groups: std::collections::BTreeMap::new(),
        new_crates: None,
        tag_template: "{{ crate }}-v{{ version }}".into(),
        recovery: None,
        journal_predecessor: None,
    };
    let mut artifact = crate::crate_release_pr_artifact::PreparationArtifact {
        schema: 1,
        binding: crate::crate_release_pr_artifact::PreparationBinding {
            repository: intent.repository.clone(),
            workflow_sha: source.clone(),
            source_sha: source.clone(),
            run_id: 42,
            run_attempt: 1,
        },
        files: vec![crate::crate_release_pr_artifact::PreparedFile {
            path: "Cargo.toml".into(),
            sha256: crate::generator::sha256_hex(b"new"),
            bytes: b"new".to_vec(),
        }],
    };
    let result = build_plan(&trees, &intent, &source, &root, &artifact)?;
    assert_eq!(result.branch, "release-plz-42-1");
    assert_eq!(result.changes["Cargo.toml"].0, "100755");
    let changed_sha = crate::release_policy_admission::git_blob_sha1(b"new")?;
    assert_eq!(
        result.expected_tree,
        git_tree_sha(&[TreeItem {
            name: "Cargo.toml".into(),
            mode: "100755".into(),
            sha: changed_sha
        }])?
    );
    artifact.files[0].path = "unauthorized/Cargo.toml".into();
    assert!(build_plan(&trees, &intent, &source, &root, &artifact).is_err());
    Ok(())
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "The complete HTTP exchange is visible in one fixture"
)]
fn native_adapter_creates_and_reconciles_through_github_http() -> Result<(), OrchestratorError> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    listener.set_nonblocking(true).expect("nonblocking");
    let address = listener.local_addr().expect("address");
    let plan = plan();
    let expected = self::plan();
    let worker = thread::spawn(move || {
        let mut branch = false;
        let mut pull = false;
        let mut requests = Vec::new();
        let mut idle = Instant::now();
        loop {
            let (mut stream, _) = match listener.accept() {
                Ok(value) => {
                    idle = Instant::now();
                    value
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && idle.elapsed() < Duration::from_secs(2) =>
                {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("accept: {e}"),
            };
            stream.set_nonblocking(false).expect("blocking stream");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("timeout");
            let mut data = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                let count = stream.read(&mut buffer).expect("read");
                assert!(count > 0);
                data.extend_from_slice(&buffer[..count]);
                if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&data[..end]).to_ascii_lowercase();
                    let len = headers
                        .lines()
                        .find_map(|v| {
                            v.strip_prefix("content-length: ")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if data.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let request = String::from_utf8(data).expect("utf8");
            let first = request.lines().next().expect("request").to_owned();
            let path = first.split_whitespace().nth(1).expect("path");
            let method = first.split_whitespace().next().expect("method");
            let is_post = method == "POST";
            let body = if path.contains("/git/ref/heads/") {
                serde_json::json!({"ref":format!("refs/heads/{}", expected.base),"object":{"type":"commit","sha":expected.source_sha}})
            } else if path.contains("/git/matching-refs/heads/") {
                if branch {
                    serde_json::json!([{"ref":format!("refs/heads/{}", expected.branch),"object":{"type":"commit","sha":"d".repeat(40)}}])
                } else {
                    serde_json::json!([])
                }
            } else if path.contains("/commits/") && path.contains("/pulls?") {
                serde_json::json!([])
            } else if path.contains("/git/commits/") {
                let email = format!(
                    "{}+github-actions[bot]@users.noreply.github.com",
                    expected.author_id
                );
                serde_json::json!({"sha":"d".repeat(40),"tree":{"sha":expected.expected_tree},"parents":[{"sha":expected.source_sha}],"message":expected.message,"author":{"name":"github-actions[bot]","email":email},"committer":{"name":"github-actions[bot]","email":email}})
            } else if path.contains("/pulls?state=open&") {
                serde_json::json!([])
            } else if path.contains("/pulls?") {
                if pull {
                    serde_json::json!([{"number":17,"base":{"repo":{"full_name":expected.repository},"ref":expected.base},"head":{"repo":{"full_name":expected.repository},"ref":expected.branch,"sha":"d".repeat(40)},"user":{"id":expected.author_id},"state":"open","title":expected.title,"body":expected.body}])
                } else {
                    serde_json::json!([])
                }
            } else if is_post && path.ends_with("/git/blobs") {
                serde_json::json!({"sha":crate::release_policy_admission::git_blob_sha1(b"new content").expect("blob")})
            } else if is_post && path.ends_with("/git/trees") {
                serde_json::json!({"sha":expected.expected_tree})
            } else if is_post && path.ends_with("/git/commits") {
                serde_json::json!({"sha":"d".repeat(40)})
            } else if is_post && path.ends_with("/git/refs") {
                branch = true;
                serde_json::json!({})
            } else if is_post && path.ends_with("/pulls") {
                pull = true;
                serde_json::json!({})
            } else {
                panic!("unexpected request: {first}")
            };
            requests.push(first);
            let encoded = body.to_string();
            write!(
                stream,
                "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                if is_post { "201 Created" } else { "200 OK" },
                encoded.len(),
                encoded
            )
            .expect("write");
        }
        requests
    });
    let github = crate::crate_release_github::NativeGitHubAdmissionRead {
        reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&format!(
            "http://{address}"
        ))?,
    };
    let writer = NativeWriter { github: &github };
    let result = create_or_reconcile(&writer, &plan);
    drop(github);
    let requests = worker.join().expect("worker");
    assert_eq!(result?, 17);
    assert_eq!(
        requests.iter().filter(|s| s.starts_with("POST ")).count(),
        5
    );
    assert!(
        requests
            .iter()
            .all(|s| s.starts_with("GET ") || s.starts_with("POST "))
    );
    Ok(())
}

#[test]
fn rejects_existing_release_pr_on_later_page_and_api_failures() -> Result<(), OrchestratorError> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };
    let first: Vec<_> = (0..100).map(|i| serde_json::json!({"head":{"ref":format!("feature-{i}"),"repo":{"full_name":"owner/project"}}})).collect();
    let replies = vec![
        ("200 OK", serde_json::json!(first)),
        (
            "200 OK",
            serde_json::json!([{"head":{"ref":"release-plz-another","repo":{"full_name":"other/fork"}}}]),
        ),
        ("403 Forbidden", serde_json::json!({"message":"denied"})),
    ];
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("address");
    let worker = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, response) in replies {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("timeout");
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).expect("read");
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            requests.push(
                String::from_utf8(bytes)
                    .expect("utf8")
                    .lines()
                    .next()
                    .expect("request")
                    .to_owned(),
            );
            let body = response.to_string();
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write");
        }
        requests
    });
    let github = crate::crate_release_github::NativeGitHubAdmissionRead {
        reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&format!(
            "http://{address}"
        ))?,
    };
    let writer = NativeWriter { github: &github };
    assert!(
        matches!(writer.require_no_other_release_pr(&plan()), Err(OrchestratorError::Internal { problem }) if problem == "release_pr_existing")
    );
    assert!(
        matches!(writer.require_no_other_release_pr(&plan()), Err(OrchestratorError::Internal { problem }) if problem == "crate_release_http_response")
    );
    let requests = worker.join().expect("worker");
    assert!(requests[0].contains("page=1"));
    assert!(requests[1].contains("page=2"));
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    Ok(())
}
