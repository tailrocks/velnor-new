// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use base64::Engine as _;
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt as _,
    path::Path,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

type Routes = Arc<Mutex<BTreeMap<String, Value>>>;
type Server = (
    String,
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
    Arc<Mutex<Vec<(String, Value)>>>,
);

struct WriterExpectation {
    repo: String,
    base: String,
    source: String,
    root: String,
    blob: String,
}

fn hash(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 15)]));
    }
    out
}
fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err("fixture git failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

struct Download {
    bytes: Vec<u8>,
    move_ref: Option<(Routes, String)>,
}
impl crate::crate_release_tool_download::ToolDownloadTransport for Download {
    fn github(
        &self,
        _: &str,
        _: u64,
        _: &str,
        _: Instant,
    ) -> Result<crate::crate_release_tool_download::ToolDownloadResponse, OrchestratorError> {
        Ok(crate::crate_release_tool_download::ToolDownloadResponse {
            status: 302,
            location: Some("https://fixture.blob.core.windows.net/artifact".into()),
            content_length: None,
            content_encoding: None,
            body: Box::new(std::io::empty()),
        })
    }
    fn storage(
        &self,
        _: &str,
        _: Instant,
    ) -> Result<crate::crate_release_tool_download::ToolDownloadResponse, OrchestratorError> {
        if let Some((routes, path)) = &self.move_ref {
            routes.lock().expect("routes").get_mut(path).expect("ref")["object"]["sha"] =
                serde_json::json!("b".repeat(40));
        }
        Ok(crate::crate_release_tool_download::ToolDownloadResponse {
            status: 200,
            location: None,
            content_length: Some(self.bytes.len() as u64),
            content_encoding: None,
            body: Box::new(std::io::Cursor::new(self.bytes.clone())),
        })
    }
}

fn tree_response(root: &Path, sha: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let output = git(root, &["ls-tree", sha])?;
    let mut entries = Vec::new();
    for row in output.lines() {
        let (meta, path) = row.split_once('\t').ok_or("tree format")?;
        let mut fields = meta.split_whitespace();
        entries.push(serde_json::json!({"mode":fields.next().ok_or("mode")?,"type":fields.next().ok_or("type")?,"sha":fields.next().ok_or("sha")?,"path":path}));
    }
    Ok(serde_json::json!({"sha":sha,"truncated":false,"tree":entries}))
}

fn server(
    routes: Routes,
    expected: WriterExpectation,
) -> Result<Server, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let running = Arc::clone(&stop);
    let log = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::clone(&log);
    let thread = thread::spawn(move || {
        let mut branch = false;
        let mut pull = false;

        while !running.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).expect("blocking stream");
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .expect("timeout");
                    let bytes = read_request(&mut stream);
                    let request = String::from_utf8_lossy(&bytes);
                    let route = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("");
                    let method = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().next())
                        .unwrap_or("");
                    let body = request.split_once("\r\n\r\n").map_or("", |(_, body)| body);
                    let payload = if body.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_str(body).expect("JSON payload")
                    };
                    events
                        .lock()
                        .expect("events")
                        .push((format!("{method} {route}"), payload.clone()));
                    let value = reply(method, route, &routes, &expected, &mut branch, &mut pull);
                    let status = if value.is_some() {
                        if method == "POST" {
                            "201 Created"
                        } else {
                            "200 OK"
                        }
                    } else {
                        "404 Not Found"
                    };
                    let body = value.unwrap_or_else(|| serde_json::json!({})).to_string();
                    write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("reply");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    Ok((url, stop, thread, log))
}

fn read_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let len = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .and_then(|n| n.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + len {
                break;
            }
        }
        let Ok(count) = stream.read(&mut buffer) else {
            break;
        };
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    bytes
}

fn reply(
    method: &str,
    route: &str,
    routes: &Routes,
    expected: &WriterExpectation,
    branch: &mut bool,
    pull: &mut bool,
) -> Option<Value> {
    let head = "d".repeat(40);
    let branch_name = "release-plz-42-1";
    let title = "chore: release";
    let description = "Automated release preparation. Review the version, dependency and changelog changes before merging.";
    if method == "GET" && route.contains("/git/matching-refs/heads/") {
        Some(if *branch {
            serde_json::json!([{"ref":format!("refs/heads/{branch_name}"),"object":{"type":"commit","sha":head}}])
        } else {
            serde_json::json!([])
        })
    } else if method == "GET" && route.contains("/commits/") && route.contains("/pulls?") {
        Some(serde_json::json!([]))
    } else if method == "GET" && route.ends_with(&format!("/git/commits/{head}")) {
        let email = "41898282+github-actions[bot]@users.noreply.github.com";
        Some(
            serde_json::json!({"sha":head,"tree":{"sha":expected.root},"parents":[{"sha":expected.source}],"message":title,"author":{"name":"github-actions[bot]","email":email},"committer":{"name":"github-actions[bot]","email":email}}),
        )
    } else if method == "GET" && route.contains("/pulls?state=open&") {
        Some(serde_json::json!([]))
    } else if method == "GET" && route.contains("/pulls?") {
        Some(if *pull {
            serde_json::json!([{"number":17,"base":{"repo":{"full_name":expected.repo},"ref":expected.base},"head":{"repo":{"full_name":expected.repo},"ref":branch_name,"sha":head},"user":{"id":41_898_282},"state":"open","title":title,"body":description}])
        } else {
            serde_json::json!([])
        })
    } else if method == "POST" && route.ends_with("/git/blobs") {
        Some(serde_json::json!({"sha":expected.blob}))
    } else if method == "POST" && route.ends_with("/git/trees") {
        Some(serde_json::json!({"sha":expected.root}))
    } else if method == "POST" && route.ends_with("/git/commits") {
        Some(serde_json::json!({"sha":head}))
    } else if method == "POST" && route.ends_with("/git/refs") {
        *branch = true;
        Some(serde_json::json!({}))
    } else if method == "POST" && route.ends_with("/pulls") {
        *pull = true;
        Some(serde_json::json!({}))
    } else if method == "GET" {
        routes.lock().expect("routes").get(route).cloned()
    } else {
        None
    }
}

struct Fixture {
    temp: tempfile::TempDir,
    sha: String,
    root: String,
    expected_tree: String,
    artifact: crate::crate_release_pr_artifact::PreparationArtifact,
    archive: Vec<u8>,
    archive_hash: String,
    content: String,
    routes: BTreeMap<String, Value>,
}

fn fixture(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    intent_text: &str,
    workflow: &str,
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".github/workflows"))?;
    std::fs::write(repo.join(".github/velnor-release-intent.toml"), intent_text)?;
    std::fs::write(repo.join(".github/workflows/release.yml"), workflow)?;
    std::fs::write(repo.join(".github/release-plz.toml"), "fixture config")?;
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname='example'\nversion='0.1.0'\n",
    )?;
    git(&repo, &["init", "-q"])?;
    git(&repo, &["add", "."])?;
    git(
        &repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    let sha = git(&repo, &["rev-parse", "HEAD"])?;
    let root = git(&repo, &["rev-parse", "HEAD^{tree}"])?;
    let binding = crate::crate_release_pr_artifact::PreparationBinding {
        repository: intent.repository.clone(),
        workflow_sha: sha.clone(),
        source_sha: sha.clone(),
        run_id: 42,
        run_attempt: 1,
    };
    let artifact = produce_artifact(&repo, temp.path(), binding, intent)?;
    let json = serde_json::to_vec(&artifact)?;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "velnor-release-pr-preparation.json",
        zip::write::SimpleFileOptions::default(),
    )?;
    zip.write_all(&json)?;
    let archive = zip.finish()?.into_inner();
    let archive_hash = hash(&archive);
    let digest = format!("sha256:{archive_hash}");
    let metadata = serde_json::json!({"id":9,"name":"velnor-release-pr-preparation-42-1","digest":digest,"size_in_bytes":archive.len(),"expired":false,"workflow_run":{"id":42,"repository_id":7,"head_repository_id":7,"head_sha":sha}});
    git(&repo, &["add", "Cargo.toml"])?;
    let expected_tree = git(&repo, &["write-tree"])?;
    git(&repo, &["reset", "--hard", "HEAD"])?;
    let routes = fixture_routes(intent, &repo, &sha, &root, metadata)?;
    Ok(Fixture {
        temp,
        sha,
        root,
        expected_tree,
        artifact,
        archive,
        archive_hash,
        content: hash(&json),
        routes,
    })
}

fn produce_artifact(
    repo: &Path,
    temp: &Path,
    binding: crate::crate_release_pr_artifact::PreparationBinding,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<crate::crate_release_pr_artifact::PreparationArtifact, Box<dyn std::error::Error>> {
    let checkout = crate::crate_release_pr_artifact::PreparationCheckout::open(
        repo,
        binding,
        intent.pr_write_paths.iter().cloned().collect(),
        crate::crate_release_git::ReleaseGit::host_fixture(),
    )?;
    let script = temp.join("fixture-updater");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf \"[package]\\nname='example'\\nversion='0.2.0'\\n\" > \"$5\"\n",
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let dir = temp.canonicalize()?;
    let env = [
        ("PATH", "/usr/bin:/bin"),
        ("CARGO", "/usr/bin/true"),
        ("RUSTC", "/usr/bin/true"),
        ("RUSTDOC", "/usr/bin/true"),
        ("HOME", dir.to_str().ok_or("temp path")?),
        ("CARGO_HOME", dir.to_str().ok_or("temp path")?),
        ("CARGO_TARGET_DIR", dir.to_str().ok_or("temp path")?),
        ("RUSTUP_HOME", dir.to_str().ok_or("temp path")?),
        ("RUSTUP_AUTO_INSTALL", "0"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect::<Vec<_>>();
    let command = velnor_actions_mise::command::IsolatedCommand::crate_update(
        &script,
        vec![
            "update".into(),
            "--config".into(),
            repo.join(".github/release-plz.toml").into_os_string(),
            "--manifest-path".into(),
            repo.join("Cargo.toml").into_os_string(),
        ],
        repo,
        &env,
    )?;
    let artifact =
        crate::crate_release_pr_artifact::fixture_execute_preparation_command(checkout, &command)?;
    assert_eq!(artifact.files.len(), 1);
    assert_eq!(artifact.files[0].path, "Cargo.toml");
    Ok(artifact)
}

fn fixture_routes(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    repo: &Path,
    sha: &str,
    root: &str,
    metadata: Value,
) -> Result<BTreeMap<String, Value>, Box<dyn std::error::Error>> {
    let gh = git(repo, &["rev-parse", "HEAD:.github"])?;
    let workflows = git(repo, &["rev-parse", "HEAD:.github/workflows"])?;
    let prefix = format!("/repos/{}", intent.repository);
    let mut routes = BTreeMap::new();
    routes.insert(prefix.clone(), serde_json::json!({"id":7,"full_name":intent.repository,"default_branch":intent.default_branch}));
    routes.insert(format!("{prefix}/actions/runs/42/attempts/1"), serde_json::json!({"id":42,"run_attempt":1,"path":".github/workflows/release.yml","event":"push","status":"in_progress","head_sha":sha,"head_branch":intent.default_branch,"repository":{"id":7,"full_name":intent.repository},"head_repository":{"id":7,"full_name":intent.repository}}));
    routes.insert(format!("{prefix}/actions/runs/42/attempts/1/jobs?per_page=100"), serde_json::json!({"total_count":1,"jobs":[{"id":5,"name":"Release preparation","run_id":42,"head_sha":sha,"status":"completed","conclusion":"success"}]}));
    routes.insert(format!("{prefix}/git/ref/heads/{}", intent.default_branch), serde_json::json!({"ref":format!("refs/heads/{}",intent.default_branch),"object":{"type":"commit","sha":sha}}));
    routes.insert(
        format!("{prefix}/git/commits/{sha}"),
        serde_json::json!({"sha":sha,"tree":{"sha":root}}),
    );
    for tree in [root, &gh, &workflows] {
        routes.insert(
            format!("{prefix}/git/trees/{tree}"),
            tree_response(repo, tree)?,
        );
    }
    for path in [
        ".github/velnor-release-intent.toml",
        ".github/workflows/release.yml",
    ] {
        let oid = git(repo, &["rev-parse", &format!("HEAD:{path}")])?;
        let bytes = std::fs::read(repo.join(path))?;
        routes.insert(format!("{prefix}/git/blobs/{oid}"), serde_json::json!({"sha":oid,"encoding":"base64","size":bytes.len(),"content":base64::engine::general_purpose::STANDARD.encode(bytes)}));
    }
    routes.insert(
        format!("{prefix}/actions/runs/42/artifacts?per_page=100"),
        serde_json::json!({"total_count":1,"artifacts":[metadata]}),
    );
    routes.insert(format!("{prefix}/actions/artifacts/9"), metadata);
    Ok(routes)
}

pub(crate) fn exercise(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    intent_text: &str,
    workflow: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Fixture {
        temp,
        sha,
        root,
        expected_tree,
        artifact,
        archive,
        archive_hash,
        content,
        routes,
    } = fixture(intent, intent_text, workflow)?;
    let prefix = format!("/repos/{}", intent.repository);
    let routes = Arc::new(Mutex::new(routes));
    let expectation = WriterExpectation {
        repo: intent.repository.clone(),
        base: intent.default_branch.clone(),
        source: sha.clone(),
        root: expected_tree.clone(),
        blob: crate::release_policy_admission::git_blob_sha1(&artifact.files[0].bytes)?,
    };
    let (url, stop, worker, events) = server(Arc::clone(&routes), expectation)?;
    let github = crate::crate_release_github::NativeGitHubAdmissionRead {
        reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&url)?,
    };
    let download = Download {
        bytes: archive,
        move_ref: None,
    };
    let runner_temp = temp.path().canonicalize()?;
    let request = AcquisitionRequest {
        binding: RunBinding {
            repository: &intent.repository,
            branch: &intent.default_branch,
            sha: &sha,
            run_id: 42,
            attempt: 1,
        },
        artifact_id: 9,
        archive_digest: &archive_hash,
        content_digest: &content,
        token: "fixture-not-a-secret",
        runner_temp: &runner_temp,
        deadline: Instant::now() + Duration::from_secs(60),
    };
    let verified = acquire_preparation(&request, &github, &download)?;
    assert_eq!(verified.repository_id, 7);
    assert_eq!(verified.intent, *intent);
    assert_eq!(verified.source_tree, root);
    assert_eq!(verified.artifact, artifact);
    let values = runtime_values(&request);
    let mut bad_values = values.clone();
    bad_values.insert("VELNOR_PR_CONTENT_SHA256", "0".repeat(64));
    let bad_context =
        crate::crate_release_pr_run::Context::from_values(|name| bad_values.get(name).cloned())?;
    assert!(crate::crate_release_pr_run::run_with(&bad_context, &github, &download).is_err());
    assert!(
        events
            .lock()
            .expect("events")
            .iter()
            .all(|(route, _)| !route.starts_with("POST "))
    );
    let context =
        crate::crate_release_pr_run::Context::from_values(|name| values.get(name).cloned())?;
    crate::crate_release_pr_run::run_with(&context, &github, &download)?;
    assert_writer_calls(&events, &artifact, &root, &expected_tree, &sha, intent)?;
    assert_rejections(&request, &github, &download, &routes, &prefix, intent);
    stop.store(true, Ordering::Relaxed);
    worker.join().map_err(|_| "server panicked")?;
    Ok(())
}

fn runtime_values(request: &AcquisitionRequest<'_>) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("GITHUB_REPOSITORY", request.binding.repository.to_owned()),
        ("GITHUB_REPOSITORY_ID", "7".to_owned()),
        ("GITHUB_EVENT_NAME", "push".to_owned()),
        ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
        (
            "GITHUB_REF",
            format!("refs/heads/{}", request.binding.branch),
        ),
        ("GITHUB_SHA", request.binding.sha.to_owned()),
        ("GITHUB_WORKFLOW_SHA", request.binding.sha.to_owned()),
        (
            "GITHUB_WORKFLOW_REF",
            format!(
                "{}/.github/workflows/release.yml@refs/heads/{}",
                request.binding.repository, request.binding.branch
            ),
        ),
        ("GITHUB_RUN_ID", "42".to_owned()),
        ("VELNOR_PR_ARTIFACT_ID", "9".to_owned()),
        (
            "VELNOR_PR_ARTIFACT_DIGEST",
            request.archive_digest.to_owned(),
        ),
        (
            "VELNOR_PR_CONTENT_SHA256",
            request.content_digest.to_owned(),
        ),
        ("GIT_TOKEN", "fixture-not-a-secret".to_owned()),
        (
            "RUNNER_TEMP",
            request.runner_temp.to_string_lossy().into_owned(),
        ),
    ])
}

fn assert_writer_calls(
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

fn assert_rejections(
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
