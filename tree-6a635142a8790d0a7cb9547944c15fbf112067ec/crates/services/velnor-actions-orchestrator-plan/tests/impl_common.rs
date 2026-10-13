//! Shared fixtures for plan integration tests, copied from the hub.
//!
//! Subset of the hub `impl_common` used by the colocated
//! plan-parity suites; the hub keeps its own copy for the
//! remaining suites.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::SystemTime;

use tempfile::TempDir;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_generation::finalized::finalized_jobs;
use velnor_actions_orchestrator_generation::prepare::GenerationPreparation;
use velnor_actions_orchestrator_plan::plan::plan_text;

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

/// Snapshot map shortcut.
pub(crate) type Snapshot = BTreeMap<String, (Vec<u8>, SystemTime)>;

/// Canonical-schema fixture for positive consumer-generation tests.
///
/// Its placeholder source and digests are serialization inputs, not
/// release provenance or qualification evidence.
pub(crate) fn fixture_manifest_json() -> String {
    include_str!("../../../../fixtures/consumer-release-manifest.json").to_owned()
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

/// Run git with inherited failure context.
pub(crate) fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
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
