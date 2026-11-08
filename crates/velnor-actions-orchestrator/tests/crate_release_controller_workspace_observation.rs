// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, Command, CrateRegistryRead, CrateRegistryResponse, CrateReleaseAction,
    CrateReleaseIntent, Digest, GitHubAdmissionRead, GitHubAdmissionRequest, Github,
    OrchestratorError, Path, RefCell, Sha256, SparseFixture, Value, err,
    fixture_verify_release_package, fs, git, json, make_fixture, prepare_crate_release, test_git,
    test_hex,
};

pub(super) fn run_response(policy_sha: &str, push: bool, legacy: bool) -> Value {
    json!({
        "id": 12,
        "run_attempt": 1,
        "head_sha": policy_sha,
        "head_branch": "main",
        "event": if push { "push" } else { "workflow_dispatch" },
        "path": if legacy {
            ".github/workflows/not-release.yml"
        } else {
            ".github/workflows/release.yml"
        },
        "repository": {"id": 22, "full_name": "example/demo", "owner": {"id": 77, "login": "example"}},
        "head_repository": {"id": 22, "full_name": "example/demo", "owner": {"id": 77, "login": "example"}}
    })
}

pub(super) fn check_runs(sha: &str, rerun_mode: u8) -> Value {
    let check = json!({"id":1,"name":"Required","head_sha":sha,"check_suite":{"id":123},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"});
    let mut checks = vec![check.clone()];
    if rerun_mode != 0 {
        let mut rerun = check;
        rerun["id"] = json!(2);
        rerun["started_at"] = json!("2026-11-03T15:41:00Z");
        if rerun_mode == 2 {
            rerun["conclusion"] = json!("failure");
        }
        checks.push(rerun);
    }
    json!({"total_count":checks.len(),"check_runs":checks})
}

pub(super) fn push_response(route: &str, sha: &str) -> Option<Value> {
    if route.contains("/pulls?per_page=100&page=1") {
        return Some(json!([{
            "number": 17,
            "merged_at": "2026-11-03T16:00:00Z",
            "merge_commit_sha": sha,
            "base": {"ref": "main", "repo": {"full_name": "example/demo", "owner": {"id": 77, "login": "example"}}},
            "head": {"ref": "release-plz-1", "repo": {"full_name": "example/demo", "owner": {"id": 77, "login": "example"}}, "sha": sha},
            "user": {"id": 54, "login": "release-bot"}
        }]));
    }
    if route.contains("/git/commits/") {
        return Some(json!({"sha": sha, "tree": {"sha": "a".repeat(40)}}));
    }
    if route.contains("/pulls/17/reviews?") {
        return Some(json!([{
            "id": 1,
            "user": {"id": 139_017, "login": "owner", "type": "User"},
            "state": "APPROVED",
            "commit_id": sha,
            "author_association": "OWNER",
            "submitted_at": "2026-11-03T15:50:00Z"
        }]));
    }
    if route.ends_with("/collaborators/owner/permission") {
        return Some(json!({"user": {"id": 139_017, "type": "User"}, "permission": "admin"}));
    }
    None
}

pub(super) struct WorkspaceGithub(pub(super) Github, Cell<bool>);
impl GitHubAdmissionRead for WorkspaceGithub {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let sha = &self.0.sha;
        if route.ends_with("/actions/runs/12") {
            return Ok(
                json!({"id":12,"run_attempt":1,"head_sha":sha,"head_branch":"main","event":"push","path":".github/workflows/release.yml","repository":{"id":22,"full_name":"example/demo","owner":{"id":77,"login":"example"}},"head_repository":{"id":22,"full_name":"example/demo","owner":{"id":77,"login":"example"}}}),
            );
        }
        if let Some(response) = push_response(route, sha) {
            return Ok(response);
        }
        self.0.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        if self.1.get() && route.contains("velnor_fixture_order_20261104_a-v0.1.1") {
            let tag = "velnor_fixture_order_20261104_a-v0.1.1";
            if route.contains("/releases/tags/") {
                return Ok(Some(
                    json!({"id":12,"tag_name":tag,"draft":false,"prerelease":false,"html_url":format!("https://github.com/example/demo/releases/tag/{tag}")}),
                ));
            }
            return Ok(Some(
                json!({"ref":format!("refs/tags/{tag}"),"object":{"type":"commit","sha":self.0.sha}}),
            ));
        }
        Ok(None)
    }
}
pub(super) struct WorkspaceRegistry {
    pub(super) visible: Cell<bool>,
    pub(super) index_visible: Cell<bool>,
    pub(super) archive: RefCell<Vec<u8>>,
}
impl CrateRegistryRead for WorkspaceRegistry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        if path == "/api/v1/users/owner?include=linked_accounts" {
            return Ok(CrateRegistryResponse { status: 200, body: br#"{"user":{"id":370532,"login":"owner"},"linked_accounts":[{"provider":"github","account_id":"139017","login":"owner"}]}"#.to_vec() });
        }
        let name = "velnor_fixture_order_20261104_a";
        if path == format!("/index/ve/ln/{name}") {
            return Ok(if self.index_visible.get() {
                CrateRegistryResponse { status: 200, body: json!({"name":name,"vers":"0.1.1","cksum":test_hex(&Sha256::digest(&*self.archive.borrow())),"yanked":false}).to_string().into_bytes() }
            } else {
                CrateRegistryResponse {
                    status: 404,
                    body: Vec::new(),
                }
            });
        }
        if self.visible.get() && path == format!("/crates/{name}/{name}-0.1.1.crate") {
            return Ok(CrateRegistryResponse {
                status: 200,
                body: self.archive.borrow().clone(),
            });
        }
        if self.visible.get() && path == format!("/api/v1/crates/{name}/0.1.1") {
            return Ok(CrateRegistryResponse {status:200,body:json!({"version":{"id":2,"crate":name,"num":"0.1.1","yanked":false,"checksum":test_hex(&Sha256::digest(&*self.archive.borrow())),"features":{"pty":[],"base":[],"default":["base"]}}}).to_string().into_bytes()});
        }
        let body = if path.ends_with("/owners") {
            json!({"users":[{"id":370_532,"login":"owner","kind":"user"}]})
        } else if path.ends_with("/0.1.1") {
            return Ok(CrateRegistryResponse {
                status: 404,
                body: Vec::new(),
            });
        } else {
            let name = path.rsplit('/').next().ok_or_else(err)?;
            if self.visible.get() && name == "velnor_fixture_order_20261104_a" {
                json!({"crate":{"id":name,"name":name,"versions":[1,2]},"versions":[{"id":1,"crate":name,"num":"0.1.0","yanked":false},{"id":2,"crate":name,"num":"0.1.1","yanked":false}]})
            } else {
                json!({"crate":{"id":name,"name":name,"versions":[1]},"versions":[{"id":1,"crate":name,"num":"0.1.0","yanked":false}]})
            }
        };
        Ok(CrateRegistryResponse {
            status: 200,
            body: body.to_string().into_bytes(),
        })
    }
}

pub(super) type WorkspaceOrderFixture = (
    tempfile::TempDir,
    CrateReleaseIntent,
    GitHubAdmissionRequest,
    WorkspaceGithub,
    WorkspaceRegistry,
);

pub(super) fn make_workspace_order_fixture()
-> Result<WorkspaceOrderFixture, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, mut request, github, _) = make_fixture(root)?;
    fs::remove_file(root.join("src/lib.rs"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['a','b']\nresolver='2'\n",
    )?;
    for name in ["a", "b"] {
        fs::create_dir_all(root.join(name).join("src"))?;
        fs::write(
            root.join(name).join("src/lib.rs"),
            if name == "a" {
                "#[cfg(not(feature = \"pty\"))] compile_error!(\"pty required\");\n#[cfg(not(feature = \"base\"))] compile_error!(\"base required\");\npub fn f() {}\n"
            } else {
                "pub fn f() {}\n"
            },
        )?;
        let dep = if name == "b" {
            "[dependencies]\nvelnor_fixture_order_20261104_a={path='../a',version='0.1.1',features=['pty']}\n"
        } else {
            ""
        };
        fs::write(
            root.join(name).join("Cargo.toml"),
            format!(
                "[package]\nname='velnor_fixture_order_20261104_{name}'\nversion='0.1.1'\nedition='2021'\ndescription='fixture'\nlicense='MIT'\n[features]\npty=[]\nbase=[]\ndefault=['base']\n{dep}"
            ),
        )?;
    }
    let lock = Command::new(env!("CARGO"))
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .output()?;
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-q", "-s", "-m", "workspace"])?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    intent.packages = vec![
        "velnor_fixture_order_20261104_a".into(),
        "velnor_fixture_order_20261104_b".into(),
    ];
    intent.publish_features = intent
        .packages
        .iter()
        .map(|name| (name.clone(), Vec::new()))
        .collect();
    intent
        .publish_features
        .insert("velnor_fixture_order_20261104_a".into(), vec!["pty".into()]);
    intent.recovery = None;
    request.event = "push".into();
    request.workflow_sha = sha.clone();
    request.source_sha = sha.clone();
    request.recovery_sha = None;
    let mut base = github;
    base.sha = sha.clone();
    base.policy_sha = sha;
    base.approved.set(true);
    let github = WorkspaceGithub(base, Cell::new(false));
    let registry = WorkspaceRegistry {
        visible: Cell::new(false),
        index_visible: Cell::new(false),
        archive: RefCell::new(Vec::new()),
    };
    Ok((temp, intent, request, github, registry))
}

#[test]
pub(super) fn multi_crate_absent_predecessor_does_not_block_observation()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, intent, request, github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert_eq!(
        admission
            .packages
            .iter()
            .map(|p| p.source.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "velnor_fixture_order_20261104_a",
            "velnor_fixture_order_20261104_b"
        ]
    );
    assert!(
        admission
            .packages
            .iter()
            .all(|p| p.action == CrateReleaseAction::Publish)
    );
    let error = fixture_verify_release_package(
        &admission,
        &intent,
        &request,
        &github,
        &registry,
        root,
        "velnor_fixture_order_20261104_b",
        test_git(),
    )
    .expect_err("unpublished predecessor must block verification");
    assert!(
        error
            .to_string()
            .contains("crate_release_package_not_ready")
    );
    fixture_verify_release_package(
        &admission,
        &intent,
        &request,
        &github,
        &registry,
        root,
        "velnor_fixture_order_20261104_a",
        test_git(),
    )?;
    verify_after_index_visibility(root, &intent, &request, &github, &registry, &admission)
}

pub(super) fn verify_after_index_visibility(
    root: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &WorkspaceGithub,
    registry: &WorkspaceRegistry,
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    let archive_dir = tempfile::tempdir()?;
    let source = admission
        .packages
        .iter()
        .find(|p| p.source.name.ends_with("_a"))
        .ok_or("a")?;
    *registry.archive.borrow_mut() = crate::crate_release_controller::package_unprivileged(
        root,
        intent,
        &source.source,
        archive_dir.path(),
        &request.source_sha,
        false,
        test_git(),
    )?;
    let server = SparseFixture::start(
        "velnor_fixture_order_20261104_a",
        registry.archive.borrow().clone(),
    )?;
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    registry.visible.set(true);
    github.1.set(true);
    let error = fixture_verify_release_package(
        admission,
        intent,
        request,
        github,
        registry,
        root,
        "velnor_fixture_order_20261104_b",
        test_git(),
    )
    .expect_err("missing index must block verification");
    assert!(error.to_string().contains("crate_release_package_failed"));
    server
        .index_visible
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let result = fixture_verify_release_package(
        admission,
        intent,
        request,
        github,
        registry,
        root,
        "velnor_fixture_order_20261104_b",
        test_git(),
    );
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    result?;
    assert!(server.index_hits.load(std::sync::atomic::Ordering::SeqCst) >= 2);
    assert!(
        server
            .download_hits
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 1
    );
    Ok(())
}

thread_local! { pub(super) static FIXTURE_SPARSE_INDEX: RefCell<Option<String>> = const { RefCell::new(None) }; }

#[path = "crate_release_controller_git_fallback.rs"]
mod git_fallback;

pub(crate) fn configure_test_registry(home: &Path) -> Result<(), OrchestratorError> {
    FIXTURE_SPARSE_INDEX.with(|value| {
        if let Some(url) = value.borrow().as_ref() {
            fs::write(home.join("config.toml"), format!("[source.crates-io]\nreplace-with='fixture'\n[source.fixture]\nregistry='sparse+{url}/'\n")).map_err(|_| OrchestratorError::Contract { problem: "fixture registry config".into() })?;
        }
        Ok(())
    })
}
