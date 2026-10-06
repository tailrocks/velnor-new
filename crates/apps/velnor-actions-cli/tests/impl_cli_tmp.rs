//! Shared spawn and tempdir helpers for CLI integration tests.

use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic counter keeping tempdir names unique within one test binary.
static COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// Best-effort tempdir removal; cleanup must never fail a test.
pub(crate) fn cleanup(dir: &Path) {
    drop(std::fs::remove_dir_all(dir));
}

/// Spawn the `velnor-actions` binary with args, env, and cwd applied.
pub(crate) fn spawn(
    args: &[&str],
    env: &[(&str, &str)],
    cwd: &Path,
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command.args(args).current_dir(cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    Ok(command.output()?)
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
    let output = Command::new("git")
        .arg("init")
        .arg("-q")
        .arg(dir)
        .output()?;
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
        let output = Command::new("git").args(&args).current_dir(dir).output()?;
        if !output.status.success() {
            return Err(format!("git {args:?} failed: {}", output.status).into());
        }
    }
    head_sha(dir)
}

/// Current HEAD sha of a fixture repo.
pub(crate) fn head_sha(dir: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
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
    git_init(dir)?;
    let output = spawn(&["init"], &[], dir)?;
    if code(&output) != 0 {
        return Err(format!("init failed with {}", code(&output)).into());
    }
    pin_branch(dir)?;
    install_consumer_manifest(dir)
}

/// Install the explicit schema fixture required by positive consumer tests.
///
/// Placeholder values in this fixture are not release evidence.
pub(crate) fn install_consumer_manifest(dir: &Path) -> Result<(), Box<dyn Error>> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/consumer-release-manifest.json");
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
