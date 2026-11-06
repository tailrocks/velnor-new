// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use base64::Engine as _;
use std::io::Write as _;

#[path = "crate_release_pr_acquisition_fixture_assertions.rs"]
mod assertions;
#[path = "crate_release_pr_acquisition_fixture_server.rs"]
mod server;
use assertions::{assert_rejections, assert_writer_calls};
use server::server;
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
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
    let api_repository = intent.repository.to_ascii_lowercase();
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
        repository: api_repository.clone(),
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
    let routes = fixture_routes(intent, &api_repository, &repo, &sha, &root, metadata)?;
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
    let command = velnor_actions_mise::command::IsolatedCommand::repo_task(
        script.to_str().ok_or("script")?,
        vec![
            "update".into(),
            "--config".into(),
            repo.join(".github/release-plz.toml").into_os_string(),
            "--manifest-path".into(),
            repo.join("Cargo.toml").into_os_string(),
        ],
        &[],
    )?
    .with_cwd(repo.to_path_buf());
    let artifact =
        crate::crate_release_pr_artifact::fixture_execute_preparation_command(checkout, &command)?;
    assert_eq!(artifact.files.len(), 1);
    assert_eq!(artifact.files[0].path, "Cargo.toml");
    Ok(artifact)
}

fn fixture_routes(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    api_repository: &str,
    repo: &Path,
    sha: &str,
    root: &str,
    metadata: Value,
) -> Result<BTreeMap<String, Value>, Box<dyn std::error::Error>> {
    let gh = git(repo, &["rev-parse", "HEAD:.github"])?;
    let workflows = git(repo, &["rev-parse", "HEAD:.github/workflows"])?;
    let prefix = format!("/repos/{api_repository}");
    let mut routes = BTreeMap::new();
    routes.insert(prefix.clone(), serde_json::json!({"id":7,"full_name":api_repository,"default_branch":intent.default_branch}));
    routes.insert(format!("{prefix}/actions/runs/42/attempts/1"), serde_json::json!({"id":42,"run_attempt":1,"path":".github/workflows/release.yml","event":"push","status":"in_progress","head_sha":sha,"head_branch":intent.default_branch,"repository":{"id":7,"full_name":api_repository},"head_repository":{"id":7,"full_name":api_repository}}));
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
    let api_repository = intent.repository.to_ascii_lowercase();
    let prefix = format!("/repos/{api_repository}");
    let routes = Arc::new(Mutex::new(routes));
    let expectation = WriterExpectation {
        repo: api_repository.clone(),
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
            repository: &api_repository,
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
