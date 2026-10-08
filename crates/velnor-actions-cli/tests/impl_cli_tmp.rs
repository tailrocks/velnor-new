//! Shared spawn and tempdir helpers for CLI integration tests.

#[path = "../../test_support/git_fixture.rs"]
pub(crate) mod git_fixture;

#[path = "impl_cli_git_isolation.rs"]
mod isolation_tests;

use std::error::Error;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic counter keeping tempdir names unique within one test binary.
static COUNTER: AtomicU64 = AtomicU64::new(0);
const NESTED_CARGO_TARGET: &str = "velnor-cli-nested-cargo";

/// Create a fresh unique directory under the system temp dir.
pub(crate) fn fresh_tempdir(prefix: &str) -> Result<PathBuf, Box<dyn Error>> {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "velnor-cli-{prefix}-{}-{id}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Give nested test-owned Cargo builds a shared target below the outer target.
/// These builds run while Nextest launches the outer test binary, so they must
/// not replace its `target/debug` executables.
pub(crate) fn nested_cargo_target_dir_for(outer_target: &Path) -> PathBuf {
    outer_target.join(NESTED_CARGO_TARGET)
}

/// Nested target for scripts that inherit Cargo's active target directory.
pub(crate) fn nested_cargo_target_dir() -> Result<PathBuf, Box<dyn Error>> {
    let outer = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            }
        }
        None => crate::impl_repo_policy::repo_root().join("target"),
    };
    std::fs::create_dir_all(&outer)?;
    Ok(nested_cargo_target_dir_for(&outer.canonicalize()?))
}

/// Acquire the process-shared lock for nested Cargo writers under `outer`.
/// The OS releases this advisory lock when the returned file is dropped or
/// its process exits, so a crashed test cannot strand a stale owner record.
pub(crate) fn lock_nested_cargo_target_for(outer: &Path) -> Result<File, Box<dyn Error>> {
    std::fs::create_dir_all(outer)?;
    let lock_path = outer
        .canonicalize()?
        .join(format!("{NESTED_CARGO_TARGET}.lock"));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.lock()?;
    Ok(lock)
}

/// Acquire the process-shared lock for scripts inheriting this test target.
pub(crate) fn lock_nested_cargo_target() -> Result<File, Box<dyn Error>> {
    let nested = nested_cargo_target_dir()?;
    let outer = nested.parent().ok_or("nested Cargo target has no parent")?;
    lock_nested_cargo_target_for(outer)
}

/// Best-effort tempdir removal; cleanup must never fail a test.
pub(crate) fn cleanup(dir: &Path) {
    if let Err(error) = std::fs::remove_dir_all(dir) {
        eprintln!(
            "failed to clean CLI test fixture {}: {error}",
            dir.display()
        );
    }
}

/// Spawn the `velnor-actions` binary with args, env, and cwd applied.
pub(crate) fn spawn(
    args: &[&str],
    env: &[(&str, &str)],
    cwd: &Path,
) -> Result<Output, Box<dyn Error>> {
    let binary = env!("CARGO_BIN_EXE_velnor-actions");
    let mut command = Command::new(binary);
    command.args(args).current_dir(cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().map_err(|error| {
        format!(
            "could not spawn {binary:?} with cwd {} (exists={}): {error}",
            cwd.display(),
            cwd.exists()
        )
        .into()
    })
}

/// Spawn with a scrubbed environment: only `PATH` plus explicit vars survive.
pub(crate) fn spawn_isolated(
    args: &[&str],
    env: &[(&str, &str)],
    cwd: &Path,
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command.args(args).current_dir(cwd).env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    Ok(command.output()?)
}

/// Initialize a Git working tree in `dir`.
pub(crate) fn git_init(dir: &Path) -> Result<(), Box<dyn Error>> {
    let output = git_fixture::command(dir)?.arg("init").arg("-q").output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("git init failed: {}", output.status).into())
    }
}

/// Exit code or -1 when the child died by signal.
pub(crate) fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

/// Commit all working-tree files with a fixed test identity; return HEAD sha.
///
/// Plan/merge verify the checkout against the request head, so protocol
/// fixtures must be real commits, not empty `git init` shells.
pub(crate) fn commit_all(dir: &Path) -> Result<String, Box<dyn Error>> {
    for args in [
        vec!["add", "-A"],
        vec![
            "-c",
            "user.name=velnor-test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    ] {
        let output = git_fixture::command(dir)?
            .args(&args)
            .current_dir(dir)
            .output()?;
        if !output.status.success() {
            return Err(format!("git {args:?} failed: {}", output.status).into());
        }
    }
    head_sha(dir)
}

/// Current HEAD sha of a fixture repo.
pub(crate) fn head_sha(dir: &Path) -> Result<String, Box<dyn Error>> {
    let output = git_fixture::command(dir)?
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        return Err("git rev-parse HEAD failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Git-init plus `init` plus branch pinning: a plannable empty repo.
pub(crate) fn init_repo(dir: &Path) -> Result<(), Box<dyn Error>> {
    git_init(dir).map_err(|error| format!("git init in {}: {error}", dir.display()))?;
    let output = spawn(&["init"], &[], dir)
        .map_err(|error| format!("run CLI init in {}: {error}", dir.display()))?;
    if code(&output) != 0 {
        return Err(format!(
            "init in {} failed with {} (stdout={:?}, stderr={:?})",
            dir.display(),
            code(&output),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    pin_branch(dir).map_err(|error| {
        format!(
            "pin branch at {} after init status={} (stdout={:?}, stderr={:?}): {error}",
            dir.join(".velnor/config.toml").display(),
            code(&output),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    install_consumer_manifest(dir).map_err(|error| {
        format!(
            "install consumer manifest into {}: {error}",
            dir.join(".velnor/release-manifest.json").display()
        )
        .into()
    })
}

/// Install the explicit schema fixture required by positive consumer tests.
///
/// Placeholder values in this fixture are not release evidence.
pub(crate) fn install_consumer_manifest(dir: &Path) -> Result<(), Box<dyn Error>> {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/consumer-release-manifest.json");
    std::fs::copy(source, dir.join(".velnor/release-manifest.json"))?;
    Ok(())
}

/// Pin the push branch so plan works without origin/HEAD.
pub(crate) fn pin_branch(repo: &Path) -> Result<(), Box<dyn Error>> {
    let config = repo.join(".velnor").join("config.toml");
    let mut body = std::fs::read_to_string(&config)?;
    body.push_str("\n[workflow]\ndefault_branch = \"main\"\n");
    std::fs::write(&config, body)?;
    Ok(())
}

/// Suppress the Rust stack after detection in an initialized repo.
pub(crate) fn ignore_rust(repo: &Path) -> Result<(), Box<dyn Error>> {
    let config = repo.join(".velnor").join("config.toml");
    let mut body = std::fs::read_to_string(&config)?;
    body.push_str("\n[stacks]\nignore = [\"rust\"]\n");
    std::fs::write(&config, body)?;
    Ok(())
}

/// Create two single-crate projects, `zebra` before `apple`, under `repo`.
pub(crate) fn add_crate_pair(repo: &Path) -> Result<(), Box<dyn Error>> {
    for name in ["zebra", "apple"] {
        let output = Command::new("cargo")
            .arg("init")
            .arg("--quiet")
            .args(["--vcs", "none"])
            .arg("--lib")
            .arg("--name")
            .arg(name)
            .arg(repo.join(name))
            .output()?;
        if !output.status.success() {
            return Err(format!("cargo init {name} failed").into());
        }
    }
    Ok(())
}

/// Add a committed-universe-ready Cargo workspace with one root crate and
/// `members` leaf crates. Used by plan protocol limits that depend on the
/// real expanded task matrix rather than serialized artifact row count.
pub(crate) fn write_workspace(repo: &Path, members: usize) -> Result<(), Box<dyn Error>> {
    let mut manifest = String::from(
        "[package]\nname = \"dynamic-root\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [workspace]\nmembers = [\n",
    );
    for index in 0..members {
        let name = format!("member{index:03}");
        writeln!(manifest, "  \"crates/{name}\",")?;
        let crate_dir = repo.join("crates").join(&name);
        std::fs::create_dir_all(crate_dir.join("src"))?;
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        std::fs::write(crate_dir.join("src/lib.rs"), "pub fn item() {}\n")?;
    }
    manifest.push_str("]\n");
    std::fs::write(repo.join("Cargo.toml"), manifest)?;
    std::fs::create_dir_all(repo.join("src"))?;
    std::fs::write(repo.join("src/lib.rs"), "pub fn root() {}\n")?;
    Ok(())
}

/// Run `plan`, requiring exit 0 and empty stderr; return stdout text.
pub(crate) fn plan_stdout(repo: &Path) -> Result<String, Box<dyn Error>> {
    let plan = spawn(&["plan"], &[], repo)?;
    if code(&plan) != 0 {
        return Err(format!("plan failed: {:?}", plan.stderr).into());
    }
    if !plan.stderr.is_empty() {
        return Err(format!("plan stderr not empty: {:?}", plan.stderr).into());
    }
    Ok(String::from_utf8_lossy(&plan.stdout).into_owned())
}

#[cfg(test)]
mod nested_target_lock_tests {
    use std::error::Error;
    use std::fs::OpenOptions;
    use std::path::Path;
    use std::process::{Child, Command, ExitStatus};
    use std::time::{Duration, Instant};

    use super::{NESTED_CARGO_TARGET, fresh_tempdir, lock_nested_cargo_target_for};

    type Outcome<T> = Result<T, Box<dyn Error>>;

    #[test]
    fn nested_target_lock_serializes_processes_and_releases_on_exit() -> Outcome<()> {
        let outer = fresh_tempdir("nested-lock")?;
        let lock_path = outer.join(format!("{NESTED_CARGO_TARGET}.lock"));
        let contender = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        let ready = outer.join("child-locked");
        let mut child = spawn_lock_child(&outer, &ready)?;
        if let Err(error) = wait_for_lock_child(&mut child, &ready) {
            let cleanup = terminate_lock_child(&mut child);
            drop(contender);
            super::cleanup(&outer);
            return match cleanup {
                Ok(_) => Err(error),
                Err(cleanup_error) => {
                    Err(format!("{error}; child cleanup failed: {cleanup_error}").into())
                }
            };
        }
        let blocked_while_child_holds_lock = contender.try_lock().is_err();
        let child_status = terminate_lock_child(&mut child)?;
        let released_after_child_exit = contender.try_lock().is_ok();
        drop(contender);
        super::cleanup(&outer);
        assert!(
            blocked_while_child_holds_lock,
            "second process acquired held lock"
        );
        assert!(
            !child_status.success(),
            "lock holder exited before termination"
        );
        assert!(
            released_after_child_exit,
            "OS did not release lock after process exit"
        );
        Ok(())
    }

    #[test]
    fn nested_target_lock_child_holds_until_killed() -> Outcome<()> {
        let Some(outer) = std::env::var_os("VELNOR_TEST_NESTED_LOCK_OUTER") else {
            return Ok(());
        };
        let _lock = lock_nested_cargo_target_for(Path::new(&outer))?;
        let ready = std::env::var_os("VELNOR_TEST_NESTED_LOCK_READY")
            .ok_or("lock child ready path is missing")?;
        std::fs::write(ready, b"locked")?;
        std::thread::park();
        Ok(())
    }

    fn spawn_lock_child(outer: &Path, ready: &Path) -> Outcome<Child> {
        Ok(Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "impl_cli_tmp::nested_target_lock_tests::nested_target_lock_child_holds_until_killed",
                "--nocapture",
            ])
            .env("VELNOR_TEST_NESTED_LOCK_OUTER", outer)
            .env("VELNOR_TEST_NESTED_LOCK_READY", ready)
            .spawn()?)
    }

    fn terminate_lock_child(child: &mut Child) -> Outcome<ExitStatus> {
        if let Err(error) = child.kill() {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            return Err(error.into());
        }
        Ok(child.wait()?)
    }

    fn wait_for_lock_child(child: &mut Child, ready: &Path) -> Outcome<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if ready.is_file() {
                return Ok(());
            }
            if let Some(status) = child.try_wait()? {
                return Err(format!("lock child exited before acquiring lock: {status}").into());
            }
            if Instant::now() >= deadline {
                return Err("lock child did not acquire lock within 10 seconds".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
