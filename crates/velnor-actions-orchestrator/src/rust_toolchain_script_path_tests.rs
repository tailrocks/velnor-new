//! The toolchain check is hermetic: it resolves every helper from
//! [`VERIFY_PATH`], never from the restored Mise `bin/` and `shims/` that
//! Setup Mise put on `PATH` ahead of the system directories.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use velnor_actions_workflow_renderer::VERIFY_PATH;

use super::super::{ToolchainPhase, toolchain_script};
use super::{Fixture, TestResult, text};

impl Fixture {
    /// Run with `ahead` on the inherited `PATH` before the fake `curl`; only
    /// the fake `curl` joins the script's own trusted `PATH`.
    pub(super) fn run_after(
        &self,
        ahead: &[PathBuf],
        phase: ToolchainPhase,
        digest: &str,
        version: &str,
    ) -> TestResult<Output> {
        let fake = self.root.join("bin");
        let script = toolchain_script(version, phase, &[(super::VERSION, digest, "2026-01-01")])
            .replacen(
                &format!("PATH={VERIFY_PATH};"),
                &format!("PATH={}:{VERIFY_PATH};", fake.display()),
                1,
            );
        let inherited = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
        let mut paths = ahead.to_vec();
        paths.push(fake);
        paths.extend(std::env::split_paths(&inherited));
        Ok(Command::new("/bin/sh")
            .arg("-c")
            .arg(&script)
            .env_clear()
            .env("PATH", std::env::join_paths(paths)?)
            .env("HOME", &self.root)
            .env("RUNNER_TEMP", self.root.join("runner-temp"))
            .env("RUSTUP_HOME", self.root.join("rustup"))
            .env("CARGO_HOME", self.root.join("cargo"))
            .env("FIXTURE_DIST", self.root.join("dist"))
            .env("FIXTURE_LOG", self.root.join("curl.log"))
            .output()?)
    }
}

/// Helpers that record they ran into `marker`, then fake success.
fn plant(dir: &Path, marker: &Path) -> TestResult {
    fs::create_dir_all(dir)?;
    for name in [
        "sha256sum",
        "shasum",
        "curl",
        "tar",
        "find",
        "awk",
        "sort",
        "cmp",
    ] {
        let path = dir.join(name);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' {name} >> '{}'\nexit 0\n",
                marker.display()
            ),
        )?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[test]
fn a_planted_helper_on_the_restored_mise_path_never_runs() -> TestResult {
    // The review's reproduction for `Verify restored Rust toolchain`: a
    // cached `~/.local/share/mise/bin/sha256sum` sat on PATH ahead of
    // `/usr/bin` and answered the digest checks itself.
    for phase in [ToolchainPhase::Restored, ToolchainPhase::Installed] {
        let fixture = Fixture::new("planted")?;
        let mise = fixture.root.join(".local/share/mise");
        let marker = fixture.root.join("planted.log");
        plant(&mise.join("bin"), &marker)?;
        plant(&mise.join("shims"), &marker)?;
        let ahead = [mise.join("shims"), mise.join("bin")];
        let output = fixture.run_after(&ahead, phase, &fixture.digest.clone(), super::VERSION)?;
        let ran = fs::read_to_string(&marker).unwrap_or_default();
        assert!(ran.is_empty(), "{phase:?}: a planted helper ran: {ran}");
        assert!(output.status.success(), "{phase:?}: {}", text(&output));
        assert!(text(&output).contains("verified file by file"), "{phase:?}");
    }
    Ok(())
}

#[test]
fn the_script_resets_path_before_any_command() {
    let script = toolchain_script("1.0.0", ToolchainPhase::Restored, &[]);
    assert!(
        script.starts_with(&format!("set -eu; PATH={VERIFY_PATH}; export PATH; ")),
        "{script}"
    );
}
