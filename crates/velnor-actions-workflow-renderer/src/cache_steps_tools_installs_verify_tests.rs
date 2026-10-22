//! The restored-install re-verification, executed: a fake Mise data dir, a
//! `mise.lock`, and release archives behind a fake `curl`.

use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use super::INSTALLS_VERIFY_SCRIPT;
use crate::VERIFY_PATH;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

static NEXT: AtomicU64 = AtomicU64::new(0);

const FAKE_CURL: &str = r#"#!/bin/sh
out=; url=
while [ $# -gt 0 ]; do
  case "$1" in --output) out=$2; shift ;; https://*) url=$1 ;; esac
  shift
done
printf '%s\n' "$url" >> "$FIXTURE_LOG"
cp "$FIXTURE_DIST/${url##*/}" "$out"
"#;

const TOOL: &str = "aqua-example-tool";
const VERSION: &str = "1.2.3";
const ARCHIVE: &str = "tool-1.2.3-x86_64-unknown-linux-gnu.tar.gz";

struct Fixture {
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.root));
    }
}

fn sha256(path: &Path) -> TestResult<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg("if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi")
        .arg("sh")
        .arg(path)
        .output()?;
    Ok(String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned())
}

fn write(path: &Path, text: &str, mode: u32) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

impl Fixture {
    /// One locked tool installed the way Mise lays it out, plus its archive.
    fn new(name: &str) -> TestResult<Self> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "velnor-installs-verify-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let fixture = Self { root };
        let staging = fixture.root.join("staging");
        write(&staging.join("tool"), "#!/bin/sh\necho genuine\n", 0o755)?;
        write(&staging.join("LICENSE"), "license\n", 0o644)?;
        fs::create_dir_all(fixture.root.join("dist"))?;
        let archive = fixture.root.join("dist").join(ARCHIVE);
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&staging)
            .args(["tool", "LICENSE"])
            .status()?;
        if !status.success() {
            return Err("tar -czf failed".into());
        }
        let dir = fixture.install();
        write(&dir.join("tool"), "#!/bin/sh\necho genuine\n", 0o755)?;
        write(&dir.join("LICENSE"), "license\n", 0o644)?;
        write(&dir.join(".mise-packslip.json"), "{}\n", 0o644)?;
        write(
            &dir.join(".mise-packslip/assets/tool.bash"),
            "complete\n",
            0o644,
        )?;
        fs::create_dir_all(dir.join(".mise-bins"))?;
        symlink(dir.join("tool"), dir.join(".mise-bins/tool"))?;
        symlink("./1.2.3", fixture.tools().join(TOOL).join("1.2"))?;
        write(
            &fixture.root.join("home/.local/share/mise/plugins/p/hook"),
            "x\n",
            0o755,
        )?;
        write(
            &fixture.root.join("home/.local/share/mise/shims/tool"),
            "x\n",
            0o755,
        )?;
        fs::create_dir_all(fixture.tools().join("rust"))?;
        symlink("/nowhere", fixture.tools().join("rust/1.99.1"))?;
        let lock = format!(
            "lockfile_version = 3\n\n[[tools.\"aqua:example/tool\"]]\nversion = \"{VERSION}\"\nbackend = \"aqua:example/tool\"\n\n[tools.\"aqua:example/tool\".\"platforms.linux-x64\"]\nchecksum = \"sha256:{}\"\nurl = \"https://github.com/example/tool/releases/download/v{VERSION}/{ARCHIVE}\"\n",
            sha256(&archive)?
        );
        write(&fixture.root.join("workspace/mise.lock"), &lock, 0o644)?;
        let curl = fixture.root.join("bin/curl");
        write(&curl, FAKE_CURL, 0o755)?;
        Ok(fixture)
    }

    fn tools(&self) -> PathBuf {
        self.root.join("home/.local/share/mise/installs")
    }

    fn install(&self) -> PathBuf {
        self.tools().join(TOOL).join(VERSION)
    }

    fn run(&self, pins: &str) -> TestResult<Output> {
        self.run_after(&[], pins)
    }

    /// Run with `ahead` on the inherited `PATH` before the fake `curl`, the
    /// way Setup Mise leaves the restored Mise `shims/` and `bin/`. Only the
    /// fake `curl` joins the script's own trusted `PATH`.
    fn run_after(&self, ahead: &[PathBuf], pins: &str) -> TestResult<Output> {
        let inherited = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
        let mut paths = ahead.to_vec();
        paths.push(self.root.join("bin"));
        paths.extend(std::env::split_paths(&inherited));
        let trusted = format!("PATH={VERIFY_PATH}\n");
        let script = INSTALLS_VERIFY_SCRIPT.replacen(
            &trusted,
            &format!("PATH={}:{VERIFY_PATH}\n", self.root.join("bin").display()),
            1,
        );
        Ok(Command::new("/bin/bash")
            .arg("-c")
            .arg(script)
            .env_clear()
            .env("PATH", std::env::join_paths(paths)?)
            .env("HOME", self.root.join("home"))
            .env("TMPDIR", &self.root)
            .env("RUNNER_OS", "Linux")
            .env("RUNNER_ARCH", "X64")
            .env("GITHUB_WORKSPACE", self.root.join("workspace"))
            .env("VELNOR_TOOL_PINS", pins)
            .env("FIXTURE_DIST", self.root.join("dist"))
            .env("FIXTURE_LOG", self.root.join("curl.log"))
            .output()?)
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The run succeeds and the install is discarded with `reason`.
fn assert_discarded(fixture: &Fixture, pins: &str, reason: &str) -> TestResult {
    let output = fixture.run(pins)?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    assert!(
        log.contains(&format!(
            "velnor: discarding restored {TOOL}@{VERSION}: {reason}"
        )),
        "{reason}:\n{log}"
    );
    assert!(!fixture.install().exists(), "{log}");
    Ok(())
}

#[test]
fn a_genuine_locked_install_is_kept_and_unpinned_code_is_dropped() -> TestResult {
    let fixture = Fixture::new("genuine")?;
    let output = fixture.run("")?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    assert!(
        log.contains(&format!("restored {TOOL}@{VERSION} matches")),
        "{log}"
    );
    assert!(fixture.install().join("tool").exists());
    assert!(fixture.tools().join(TOOL).join("1.2").is_symlink());
    let mise = fixture.root.join("home/.local/share/mise");
    assert!(!mise.join("plugins").exists() && !mise.join("shims").exists());
    assert!(
        fixture.tools().join("rust/1.99.1").is_symlink(),
        "Rust is left to its own check"
    );
    Ok(())
}

#[test]
fn a_replaced_binary_is_discarded() -> TestResult {
    // The cache-poisoning case: a default-branch job rewrote a cached tool.
    let fixture = Fixture::new("replaced")?;
    fs::write(fixture.install().join("tool"), "#!/bin/sh\necho pwned\n")?;
    assert_discarded(&fixture, "", "a file differs from the pinned archive")
}

#[test]
fn a_planted_file_is_discarded() -> TestResult {
    let fixture = Fixture::new("planted")?;
    write(&fixture.install().join("helper"), "#!/bin/sh\n", 0o755)?;
    assert_discarded(&fixture, "", "a file differs from the pinned archive")
}

#[test]
fn an_escaping_symlink_is_discarded() -> TestResult {
    let fixture = Fixture::new("escape")?;
    fs::remove_file(fixture.install().join(".mise-bins/tool"))?;
    symlink("/bin/sh", fixture.install().join(".mise-bins/tool"))?;
    assert_discarded(&fixture, "", "symlink .mise-bins/tool leaves the install")
}

#[test]
fn executable_bookkeeping_is_discarded() -> TestResult {
    let fixture = Fixture::new("bookkeeping")?;
    write(
        &fixture.install().join(".mise-packslip/assets/run.sh"),
        "#!/bin/sh\n",
        0o755,
    )?;
    assert_discarded(&fixture, "", "executable file in Mise bookkeeping")
}

#[test]
fn an_archive_that_differs_from_the_lock_is_discarded() -> TestResult {
    let fixture = Fixture::new("archive")?;
    fs::write(fixture.root.join("dist").join(ARCHIVE), "substitute")?;
    assert_discarded(
        &fixture,
        "",
        "archive does not match the mise.lock checksum",
    )
}

#[test]
fn an_install_without_any_pin_is_discarded() -> TestResult {
    let fixture = Fixture::new("unpinned")?;
    fs::write(
        fixture.root.join("workspace/mise.lock"),
        "lockfile_version = 3\n",
    )?;
    assert_discarded(&fixture, "", "no mise.lock or catalog pin for linux-x64")
}

#[test]
fn a_catalog_pin_verifies_a_tool_the_lock_does_not_name() -> TestResult {
    let fixture = Fixture::new("catalog")?;
    fs::write(
        fixture.root.join("workspace/mise.lock"),
        "lockfile_version = 3\n",
    )?;
    let pin = format!(
        "{TOOL} {VERSION} linux-x64 https://github.com/example/tool/releases/download/v{VERSION}/{ARCHIVE} {}",
        sha256(&fixture.root.join("dist").join(ARCHIVE))?
    );
    let output = fixture.run(&format!(
        "other 1.0.0 macos-arm64 https://x.invalid/a {}\n{pin}",
        "0".repeat(64)
    ))?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    assert!(
        log.contains(&format!("restored {TOOL}@{VERSION} matches")),
        "{log}"
    );
    assert!(fixture.install().join("tool").exists());
    Ok(())
}

#[test]
fn an_alias_that_leaves_its_tool_is_removed() -> TestResult {
    let fixture = Fixture::new("alias")?;
    symlink("/tmp", fixture.tools().join(TOOL).join("latest"))?;
    let output = fixture.run("")?;
    assert!(output.status.success(), "{}", text(&output));
    assert!(!fixture.tools().join(TOOL).join("latest").exists());
    assert!(fixture.tools().join(TOOL).join("1.2").is_symlink());
    Ok(())
}

/// Write helpers that record they ran into `marker`, then fake success.
fn plant(dir: &Path, names: &[&str], marker: &Path) -> TestResult {
    for name in names {
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n' {name} >> '{}'\nexit 0\n",
            marker.display()
        );
        write(&dir.join(name), &body, 0o755)?;
    }
    Ok(())
}

#[test]
fn a_planted_helper_on_the_restored_mise_path_never_runs() -> TestResult {
    // The review's reproduction: the restored `bin/` and `shims/` sit on
    // PATH ahead of `/usr/bin`, so a cached `bin/sha256sum` ran inside the
    // very check meant to gate it.
    let fixture = Fixture::new("planted-helper")?;
    let mise = fixture.root.join("home/.local/share/mise");
    let marker = fixture.root.join("planted.log");
    let helpers = [
        "sha256sum",
        "shasum",
        "curl",
        "tar",
        "find",
        "awk",
        "cut",
        "sort",
        "comm",
    ];
    plant(&mise.join("bin"), &helpers, &marker)?;
    plant(&mise.join("shims"), &helpers, &marker)?;
    let output = fixture.run_after(&[mise.join("shims"), mise.join("bin")], "")?;
    let log = text(&output);
    let ran = fs::read_to_string(&marker).unwrap_or_default();
    assert!(ran.is_empty(), "a planted helper ran: {ran}\n{log}");
    assert!(output.status.success(), "{log}");
    assert!(
        log.contains(&format!("restored {TOOL}@{VERSION} matches")),
        "{log}"
    );
    Ok(())
}

#[test]
fn a_hidden_version_directory_behind_an_alias_is_discarded() -> TestResult {
    // The review's reproduction: `installs/<tool>/.hidden/` (bytes from no
    // archive) plus `<pinned version> -> ./.hidden` passed, because the glob
    // skipped the dot directory and the alias branch kept any `./*` target.
    let fixture = Fixture::new("hidden")?;
    let tool = fixture.tools().join(TOOL);
    fs::remove_dir_all(fixture.install())?;
    write(&tool.join(".hidden/tool"), "#!/bin/sh\necho pwned\n", 0o755)?;
    symlink("./.hidden", fixture.install())?;
    let hidden_tool = fixture.tools().join(".hidden-tool/1.0.0");
    write(&hidden_tool.join("tool"), "#!/bin/sh\n", 0o755)?;
    let output = fixture.run("")?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    assert!(!tool.join(".hidden").exists(), "{log}");
    assert!(fs::symlink_metadata(fixture.install()).is_err(), "{log}");
    assert!(fs::symlink_metadata(tool.join("1.2")).is_err(), "{log}");
    assert!(!hidden_tool.exists(), "{log}");
    Ok(())
}

#[test]
fn only_aliases_of_a_verified_sibling_and_inert_bookkeeping_are_kept() -> TestResult {
    let fixture = Fixture::new("aliases")?;
    let tool = fixture.tools().join(TOOL);
    let backend = tool.join(".mise.backend.toml");
    write(&backend, "backend = \"aqua:example/tool\"\n", 0o644)?;
    write(&tool.join(".run"), "#!/bin/sh\n", 0o755)?;
    symlink("./1.2", tool.join("chain"))?;
    symlink("1.2.3", tool.join("bare"))?;
    symlink("./1.2.3/tool", tool.join("into"))?;
    let output = fixture.run("")?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    assert!(backend.is_file(), "{log}");
    assert!(tool.join("1.2").is_symlink(), "{log}");
    assert!(tool.join("bare").is_symlink(), "{log}");
    for gone in [".run", "chain", "into"] {
        assert!(
            fs::symlink_metadata(tool.join(gone)).is_err(),
            "{gone}: {log}"
        );
    }
    Ok(())
}
