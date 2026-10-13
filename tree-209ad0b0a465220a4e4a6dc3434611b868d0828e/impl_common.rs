//! Shared fixtures and helpers for orchestrator integration tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::SystemTime;

use tempfile::TempDir;
use velnor_actions_contract::{MatrixReport, Plan};
use velnor_actions_orchestrator::{OrchestratorError, plan_internal};

/// Test error shortcut.
pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Snapshot map shortcut.
pub(crate) type Snapshot = BTreeMap<String, (Vec<u8>, SystemTime)>;

/// Release-manifest fixture for consumer generation tests.
///
/// Both debug and release builds read `.velnor/release-manifest.json`
/// as consumer provenance. Every fixture repo carries it so consumer
/// `prepare` succeeds.
pub(crate) fn fixture_manifest_json() -> String {
    let targets = [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ]
    .iter()
    .map(|target| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://example.invalid/r/{target}\",\"sha256\":\"{}\"}}",
            "a".repeat(64)
        )
    })
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{targets}]}}"
    )
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
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
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
    let status = StdCommand::new("git")
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
    let output = StdCommand::new("git")
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
