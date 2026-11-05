// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, Command, CrateRegistryRead, CrateRegistryResponse, CrateReleaseAction,
    CrateReleaseIntent, Digest, GitHubAdmissionRequest, Github, OrchestratorError, Path, RefCell,
    Sha256, fs, json, prepare_crate_release, repair_crate_release, test_git,
};

pub(super) struct NewRegistry {
    pub(super) base: Registry,
    pub(super) visible: Cell<bool>,
    pub(super) owner_changed: Cell<bool>,
}
impl CrateRegistryRead for NewRegistry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        if !self.visible.get() && path.starts_with("/api/v1/crates/demo") {
            return Ok(CrateRegistryResponse {
                status: 404,
                body: vec![],
            });
        }
        if self.visible.get() && self.owner_changed.get() && path == "/api/v1/crates/demo/owners" {
            return Ok(CrateRegistryResponse {
                status: 200,
                body: br#"{"users":[{"id":370533,"login":"owner","kind":"user"}]}"#.to_vec(),
            });
        }
        if self.visible.get() && path == "/api/v1/crates/demo" {
            return Ok(CrateRegistryResponse { status: 200, body: json!({"crate":{"id":"demo","name":"demo","versions":[2]},"versions":[{"id":2,"crate":"demo","num":"0.1.1","yanked":false}]}).to_string().into_bytes() });
        }
        self.base.get(path)
    }
}

pub(super) struct Registry {
    pub(super) absent: Cell<bool>,
    pub(super) inconsistent: Cell<bool>,
    pub(super) archive: RefCell<Vec<u8>>,
}
impl CrateRegistryRead for Registry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        if path == "/api/v1/crates/demo/owners" {
            return Ok(CrateRegistryResponse {
                status: 200,
                body: br#"{"users":[{"id":370532,"login":"owner","kind":"user"}]}"#.to_vec(),
            });
        }
        if path == "/api/v1/crates/demo" {
            let mut versions = vec![json!({"id":1,"crate":"demo","num":"0.1.0","yanked":false})];
            if !self.absent.get() && !self.inconsistent.get() {
                versions.push(json!({"id":2,"crate":"demo","num":"0.1.1","yanked":false}));
            }
            let ids: Vec<_> = versions
                .iter()
                .map(|version| version["id"].clone())
                .collect();
            return Ok(CrateRegistryResponse {
                status: 200,
                body:
                    json!({"crate":{"id":"demo","name":"demo","versions":ids},"versions":versions})
                        .to_string()
                        .into_bytes(),
            });
        }
        if path == "/api/v1/crates/demo/0.1.1" {
            if self.absent.get() {
                return Ok(CrateRegistryResponse {
                    status: 404,
                    body: Vec::new(),
                });
            }
            let digest = Sha256::digest(&*self.archive.borrow());
            let mut checksum = String::new();
            for byte in digest {
                checksum.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                checksum.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
            }
            return Ok(CrateRegistryResponse { status:200, body:json!({"version":{"id":2,"crate":"demo","num":"0.1.1","checksum":checksum,"yanked":false,"features":{"demo-feature":[]}}}).to_string().into_bytes() });
        }
        if path == "/crates/demo/demo-0.1.1.crate" {
            return Ok(CrateRegistryResponse {
                status: 200,
                body: self.archive.borrow().clone(),
            });
        }
        Err(OrchestratorError::Contract {
            problem: "unexpected registry route".to_owned(),
        })
    }
}
pub(super) fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let result = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

pub(super) fn make_fixture(
    root: &Path,
) -> Result<
    (CrateReleaseIntent, GitHubAdmissionRequest, Github, Registry),
    Box<dyn std::error::Error>,
> {
    git(root, &["init", "-q"])?;
    git(root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(root, &["config", "user.email", "alexey@zhokhov.com"])?;
    git(root, &["config", "commit.gpgsign", "false"])?;
    git(root, &["config", "core.hooksPath", "/dev/null"])?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn demo() {}")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.1\"\nedition = \"2021\"\ndescription = \"fixture\"\nlicense = \"MIT\"\n[features]\ndemo-feature = []\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.1\"\n",
    )?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "fixture",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let rust = catalog.version(velnor_actions_mise::PinnedTool::Rust);
    let intent: CrateReleaseIntent = serde_json::from_value(
        json!({"schema":1,"repository":"example/demo","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"rust_version":rust,"generator_version":"0.1.72","generator_source_sha":"f1041f322c54cd7edfdef06afd307799eeaa801a","generator_manifest_sha256":"e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52","release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":{"package":"demo","version":"0.1.1","source_sha":sha}}),
    )?;
    let request = GitHubAdmissionRequest {
        repository: "example/demo".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        trusted_reviewer_id: 139_017,
        workflow_path: ".github/workflows/release.yml".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        workflow_sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        run_id: 12,
        run_attempt: 1,
        event: "workflow_dispatch".into(),
        source_sha: sha.clone(),
        recovery_sha: Some(sha.clone()),
    };
    let github = Github {
        sha,
        policy_sha: request.workflow_sha.clone(),
        approved: Cell::new(false),
        revoke_on_tag_read: Cell::new(false),
        tag_exists: Cell::new(false),
        tag_wrong: Cell::new(false),
        owner_match: Cell::new(true),
        release_variant: Cell::new(0),
        write_error: Cell::new(0),
        writes: Cell::new(0),
        tool_archive: RefCell::new(Vec::new()),
        mutate_after_download: Cell::new(0),
        artifact_receipt: Cell::new(false),
        before_metadata_write: RefCell::new(None),
    };
    let registry = Registry {
        absent: Cell::new(true),
        inconsistent: Cell::new(false),
        archive: RefCell::new(Vec::new()),
    };
    Ok((intent, request, github, registry))
}

pub(super) fn fixture_archive(root: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let target = tempfile::tempdir()?;
    let tool =
        velnor_actions_mise::ToolCatalog::pinned().tool_spec(velnor_actions_mise::PinnedTool::Rust);
    let which = Command::new("mise")
        .current_dir(target.path())
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "which",
            "cargo",
            "--tool",
            &tool,
        ])
        .output()?;
    if !which.status.success() {
        return Err("cannot resolve pinned cargo".into());
    }
    let cargo = String::from_utf8(which.stdout)?.trim().to_owned();
    let package = Command::new("mise")
        .current_dir(root)
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            &tool,
            "--",
            &cargo,
            "package",
            "--locked",
            "--offline",
            "--no-verify",
            "--target-dir",
        ])
        .arg(target.path())
        .output()?;
    if !package.status.success() {
        return Err("fixture package failed".into());
    }
    Ok(fs::read(target.path().join("package/demo-0.1.1.crate"))?)
}

pub(super) fn exercise_repair(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &Github,
    registry: &Registry,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let repair_tag = prepare_crate_release(intent, request, github, registry, root)?;
    let attempt = || {
        repair_crate_release(
            &repair_tag,
            intent,
            request,
            github,
            registry,
            root,
            test_git(),
        )
    };
    assert!(matches!(
        repair_tag.packages[0].action,
        CrateReleaseAction::RepairTag { .. }
    ));
    github.approved.set(false);
    assert!(attempt().is_err());
    assert_eq!(github.writes.get(), 0);
    github.approved.set(true);
    github.owner_match.set(false);
    assert!(attempt().is_err());
    assert_eq!(github.writes.get(), 0);
    github.owner_match.set(true);
    registry.archive.borrow_mut().push(0);
    assert!(attempt().is_err());
    assert_eq!(github.writes.get(), 0);
    registry.archive.borrow_mut().pop();
    github.release_variant.set(4);
    github.tag_exists.set(true);
    github.tag_wrong.set(true);
    assert!(attempt().is_err());
    assert_eq!(github.writes.get(), 0);
    github.tag_wrong.set(false);
    github.tag_exists.set(false);
    github.write_error.set(1);
    assert!(attempt().is_err());
    assert!(!github.tag_exists.get());
    assert_eq!(github.writes.get(), 1);
    github.write_error.set(3);
    assert!(attempt().is_err());
    assert!(github.tag_exists.get());
    assert_eq!(github.release_variant.get(), 4);
    github.write_error.set(0);
    let after_timeout = prepare_crate_release(intent, request, github, registry, root)?;
    assert!(matches!(
        after_timeout.packages[0].action,
        CrateReleaseAction::RepairRelease { .. }
    ));
    github.tag_exists.set(false);
    attempt()?;
    assert!(github.tag_exists.get());
    assert_eq!(github.release_variant.get(), 0);
    github.tag_exists.set(false);

    github.tag_exists.set(true);
    github.release_variant.set(4);
    let repair_release = prepare_crate_release(intent, request, github, registry, root)?;
    assert!(matches!(
        repair_release.packages[0].action,
        CrateReleaseAction::RepairRelease { .. }
    ));
    github.release_variant.set(0);
    let already_complete = prepare_crate_release(intent, request, github, registry, root)?;
    assert!(matches!(
        already_complete.packages[0].action,
        CrateReleaseAction::Reconciled { .. }
    ));
    Ok(())
}
