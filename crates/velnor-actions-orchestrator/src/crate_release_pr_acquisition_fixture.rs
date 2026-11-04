// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use base64::Engine as _;
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
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
type Server = (String, Arc<AtomicBool>, thread::JoinHandle<()>);

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

fn server(routes: Routes) -> Result<Server, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let running = Arc::clone(&stop);
    let thread = thread::spawn(move || {
        while !running.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).expect("blocking stream");
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .expect("timeout");
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 4096];
                    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        let Ok(count) = stream.read(&mut buffer) else {
                            break;
                        };
                        if count == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    let request = String::from_utf8_lossy(&bytes);
                    let route = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("");
                    let value = routes.lock().expect("routes").get(route).cloned();
                    let status = if value.is_some() {
                        "200 OK"
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
    Ok((url, stop, thread))
}

struct Fixture {
    temp: tempfile::TempDir,
    sha: String,
    root: String,
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
    let changed = b"[package]\nname='example'\nversion='0.2.0'\n".to_vec();
    let artifact = crate::crate_release_pr_artifact::PreparationArtifact {
        schema: 1,
        binding,
        files: vec![crate::crate_release_pr_artifact::PreparedFile {
            path: "Cargo.toml".into(),
            sha256: hash(&changed),
            bytes: changed,
        }],
    };
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
    let routes = fixture_routes(intent, &repo, &sha, &root, metadata)?;
    Ok(Fixture {
        temp,
        sha,
        root,
        artifact,
        archive,
        archive_hash,
        content: hash(&json),
        routes,
    })
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
        artifact,
        archive,
        archive_hash,
        content,
        routes,
    } = fixture(intent, intent_text, workflow)?;
    let prefix = format!("/repos/{}", intent.repository);
    let routes = Arc::new(Mutex::new(routes));
    let (url, stop, worker) = server(Arc::clone(&routes))?;
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
    assert_rejections(&request, &github, &download, &routes, &prefix, intent);
    stop.store(true, Ordering::Relaxed);
    worker.join().map_err(|_| "server panicked")?;
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
