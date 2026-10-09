//! Shared fixtures and helpers for orchestrator integration tests.

#[path = "../../test_support/git_fixture.rs"]
pub(crate) mod git_fixture;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::SystemTime;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use velnor_actions_contract::{MatrixReport, Plan};
use velnor_actions_orchestrator::{
    GenerationPreparation, OrchestratorError, finalized_jobs, plan_internal, plan_text,
};

/// Test error shortcut.
pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Plan text over the same finalized jobs `generate` writes.
///
/// Tests asserting plan content go through this helper so they cover
/// the plan/generate parity path (validators included) instead of a
/// pre-merge IR projection no user ever sees.
pub(crate) fn plan_for(prep: &GenerationPreparation) -> Result<String, Box<dyn std::error::Error>> {
    Ok(plan_text(prep, &finalized_jobs(prep)?))
}

/// Marker proving the current process already scrubbed ambient identity.
const SCRUBBED_ENV: &str = "VELNOR_TEST_SCRUBBED_IDENTITY";

/// Anchor-bearing CI vars scrubbed for local-semantics merge tests.
const ANCHOR_ENV_VARS: [&str; 5] = [
    "GITHUB_REPOSITORY",
    "GITHUB_BASE_REF",
    "GITHUB_REF",
    "GITHUB_WORKFLOW_REF",
    "GITHUB_ACTIONS",
];

/// True when ambient `GITHUB_REPOSITORY` names a non-canonical repo.
///
/// Fork CI exports the fork identity; Velnor-policy fixtures carry the
/// canonical origin, so an in-process `prepare` would fail closed on
/// the ambient hint instead of exercising the fixture.
pub(crate) fn ambient_identity_blocks() -> bool {
    std::env::var("GITHUB_REPOSITORY")
        .is_ok_and(|hint| hint.to_lowercase() != "tailrocks/velnor-new")
}

/// Run Velnor-policy assertions with ambient identity scrubbed.
///
/// When [`ambient_identity_blocks`], re-executes the calling test in a
/// child with `GITHUB_REPOSITORY` removed (child env needs no `unsafe`,
/// which `unsafe_code = "forbid"` bars even in tests) and requires
/// exactly one passing child run — a missing or failing child fails
/// loudly, never green. Otherwise runs `inner` in-process. `test` is
/// the bare test name, which must be unique in the binary.
pub(crate) fn without_ambient_identity(
    test: &str,
    inner: impl FnOnce() -> TestResult,
) -> TestResult {
    if std::env::var(SCRUBBED_ENV).is_err() && ambient_identity_blocks() {
        let output = StdCommand::new(std::env::current_exe()?)
            .arg(test)
            .env(SCRUBBED_ENV, "1")
            .env_remove("GITHUB_REPOSITORY")
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{test}: {stdout}{stderr}");
        assert!(stdout.contains("1 passed"), "{test}: {stdout}");
        return Ok(());
    }
    inner()
}

/// Run merge assertions with ambient CI anchors scrubbed.
///
/// Merge revalidates covered claims against runner ground truth, so a
/// fixture manifest (anchored to `o/r`) disagrees with any ambient CI
/// identity — canonical included. When any anchor var is set,
/// re-executes the calling test in a child with all five removed
/// (child env needs no `unsafe`, barred even in tests) and requires
/// exactly one passing child run. Otherwise runs `inner` in-process.
/// `test` is the bare test name, which must be unique in the binary.
/// Anchor enforcement itself is covered hermetically by the pure
/// `revalidate_coverage_with_anchors` unit tests.
pub(crate) fn without_ambient_ci_env(test: &str, inner: impl FnOnce() -> TestResult) -> TestResult {
    let anchored = ANCHOR_ENV_VARS
        .iter()
        .any(|var| std::env::var_os(var).is_some());
    if std::env::var(SCRUBBED_ENV).is_err() && anchored {
        let mut command = StdCommand::new(std::env::current_exe()?);
        command.arg(test).env(SCRUBBED_ENV, "1");
        for var in ANCHOR_ENV_VARS {
            command.env_remove(var);
        }
        let output = command.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{test}: {stdout}{stderr}");
        assert!(stdout.contains("1 passed"), "{test}: {stdout}");
        return Ok(());
    }
    inner()
}

/// Snapshot map shortcut.
pub(crate) type Snapshot = BTreeMap<String, (Vec<u8>, SystemTime)>;

/// Runtime-synthetic current-version manifest for disposable ConsumerV1 tests.
///
/// The canonical asset URL shape satisfies the contract, but the source marker
/// and per-target digests come from deterministic test payloads. This manifest
/// is installed only in temporary repositories; it is not release evidence.
pub(crate) fn fixture_manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            let payload = format!("Velnor synthetic test payload; version={version}; target={target}\n");
            let digest = synthetic_sha256(payload.as_bytes());
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{digest}\"}}"
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let source_marker = synthetic_sha256(b"Velnor synthetic test source marker");
    let commit = source_marker.chars().take(40).collect::<String>();
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
    )
}

fn synthetic_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

/// Install the deterministic schema-only manifest in a positive fixture.
pub(crate) fn install_fixture_release_manifest(root: &Path) -> TestResult {
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    Ok(())
}

/// Build a git fixture: config plus one root crate (uncommitted).
pub(crate) fn make_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    install_fixture_release_manifest(root)?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

/// Minimal valid config with an explicit branch (no git branch lookup).
pub(crate) fn config_with_branch() -> &'static str {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n"
}

/// Nextest task file so the fixture detects the Nextest runner.
pub(crate) fn write_nextest_task(root: &Path) -> TestResult {
    fs::create_dir_all(root.join(".mise/tasks"))?;
    let task = root.join(".mise/tasks/test");
    fs::write(&task, "#!/bin/sh\ncargo nextest run --locked\n")?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&task)?.permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        fs::set_permissions(&task, perms)?;
    }
    Ok(())
}

/// Run git with inherited failure context.
pub(crate) fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = git_fixture::command(cwd)?
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
}

/// Anchor a fixture repo so fail-closed provenance can validate it.
pub(crate) fn anchor_repo(root: &Path) -> TestResult {
    git(
        &["remote", "add", "origin", "https://github.com/o/r.git"],
        root,
    )
}

/// Manifest `repository_id` matching [`anchor_repo`].
pub(crate) fn anchor_id() -> String {
    velnor_actions_contract::digest_b3(b"github.com/o/r")
}

/// Single git stdout line.
pub(crate) fn git_line(args: &[&str], cwd: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = git_fixture::command(cwd)?
        .args(args)
        .current_dir(cwd)
        .output()?;
    assert!(output.status.success(), "git {args:?} failed");
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Snapshot every regular file: relative path to bytes plus mtime.
pub(crate) fn snapshot(root: &Path) -> Result<Snapshot, Box<dyn std::error::Error>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<PathBuf> = Vec::new();
        for entry in fs::read_dir(&dir)? {
            entries.push(entry?.path());
        }
        entries.sort();
        for path in entries {
            let meta = fs::symlink_metadata(&path)?;
            if meta.is_dir() && !meta.is_symlink() {
                stack.push(path);
            } else if meta.is_file() {
                let rel = path.strip_prefix(root)?.display().to_string();
                let bytes = fs::read(&path)?;
                out.insert(rel, (bytes, meta.modified()?));
            }
        }
    }
    Ok(out)
}

/// Unwrap the error side or fail the test.
pub(crate) fn err_of<T>(
    result: Result<T, OrchestratorError>,
    what: &str,
) -> Result<OrchestratorError, Box<dyn std::error::Error>> {
    result.err().ok_or_else(|| {
        Box::new(std::io::Error::other(format!("{what}: expected error")))
            as Box<dyn std::error::Error>
    })
}

/// Restores write permission on drop so tempdir cleanup succeeds.
pub(crate) struct ReadonlyGuard {
    /// Guarded path.
    path: PathBuf,
    /// Permissions to restore.
    perms: fs::Permissions,
}

impl ReadonlyGuard {
    /// Make `path` read-only.
    pub(crate) fn lock(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let perms = fs::metadata(path)?.permissions();
        let mut frozen = perms.clone();
        frozen.set_readonly(true);
        fs::set_permissions(path, frozen)?;
        Ok(Self {
            path: path.to_path_buf(),
            perms,
        })
    }
}

impl Drop for ReadonlyGuard {
    fn drop(&mut self) {
        if fs::set_permissions(&self.path, self.perms.clone()).is_err() {
            // Best effort; tempdir cleanup reports failures itself.
        }
    }
}

/// Plan for a two-commit repo whose second commit touches crate sources.
pub(crate) fn plan_for_source_change() -> Result<(TempDir, Plan), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(value["schema"], 1);
    assert_eq!(
        value["matrix"], value["plan"]["matrix"],
        "matrix byte-agreement"
    );
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok((repo, plan))
}

/// One passing matrix report per plan entry.
pub(crate) fn passing_reports(
    plan: &Plan,
) -> Result<Vec<MatrixReport>, Box<dyn std::error::Error>> {
    let mut reports: Vec<MatrixReport> = Vec::new();
    for entry in &plan.matrix.include {
        let obligation = plan
            .obligations
            .iter()
            .find(|ob| ob.task_id == entry.task_id)
            .ok_or_else(|| std::io::Error::other("missing obligation"))?;
        let task_report_id = velnor_actions_contract::task_report_id_for_task(
            "local",
            &entry.matrix_key,
            &obligation.task_digest,
        )?;
        reports.push(MatrixReport {
            schema: 1,
            report_id: entry.report_id.clone(),
            run_key: "local".to_owned(),
            matrix_id: entry.id.clone(),
            matrix_key: entry.matrix_key.clone(),
            status: velnor_actions_contract::MatrixStatus::Passed,
            expected_task_ids: vec![entry.task_id.clone()],
            task_report_ids: vec![task_report_id.clone()],
            tasks: vec![velnor_actions_contract::MatrixTaskEntry {
                task_report_id,
                task_id: entry.task_id.clone(),
                status: velnor_actions_contract::TaskStatus::Executed,
                exit_code: 0,
            }],
            selected: 1,
            reused: 0,
            executed: 1,
            empty_partition: 0,
            not_selected: 0,
            failed: 0,
            cancelled: 0,
        });
    }
    for report in &reports {
        report.validate()?;
    }
    Ok(reports)
}

/// Build a git fixture: config plus one virtual workspace (uncommitted).
///
/// The root manifest carries `[workspace]` only, so no member owns it;
/// member `a` lives under `crates/a`.
pub(crate) fn make_virtual_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = make_repo(config)?;
    let root = dir.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/a\"]\n",
    )?;
    fs::remove_dir_all(root.join("src"))?;
    let member = root.join("crates/a");
    fs::create_dir_all(member.join("src"))?;
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(member.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}
