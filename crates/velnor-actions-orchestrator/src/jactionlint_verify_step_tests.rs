//! Behaviour of the generated `Verify jactionlint` script under a fake Mise.

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use velnor_actions_mise::{JACTIONLINT_VERSION, PinnedTool, ToolCatalog, jactionlint_executable_sha256};

use super::{NOT_CATALOGUED_CODE, VERIFY_JACTIONLINT_STEP, script, verify_jactionlint_step};

const FAKE_BYTES: &[u8] = b"fake jactionlint executable\n";

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A temp root with `bin/mise` printing the fake executable's path.
fn fake_runner(root: &Path) -> Result<(), Box<dyn Error>> {
    let bin = root.join("bin");
    fs::create_dir_all(&bin)?;
    let exe = root.join("jactionlint");
    fs::write(&exe, FAKE_BYTES)?;
    let mise = bin.join("mise");
    fs::write(
        &mise,
        format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{log}\"\nprintf '%s\\n' \"{exe}\"\n",
            log = root.join("mise.log").display(),
            exe = exe.display()
        ),
    )?;
    let mut permissions = fs::metadata(&mise)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&mise, permissions)?;
    Ok(())
}

fn run(root: &Path, script: &str, os: &str, arch: &str) -> Result<Output, Box<dyn Error>> {
    let path = format!("{}:{}", root.join("bin").display(), std::env::var("PATH")?);
    Ok(Command::new("sh")
        .arg("-c")
        .arg(script)
        .env("PATH", path)
        .env("RUNNER_TEMP", root)
        .env("RUNNER_OS", os)
        .env("RUNNER_ARCH", arch)
        .output()?)
}

fn pinned_digest(platform: &str) -> Result<&'static str, Box<dyn Error>> {
    jactionlint_executable_sha256(JACTIONLINT_VERSION, platform)
        .ok_or_else(|| format!("no {platform} digest").into())
}

#[test]
fn a_matching_executable_passes_for_every_runner_platform() -> Result<(), Box<dyn Error>> {
    for (os, arch, platform) in [
        ("Linux", "X64", "linux-x64"),
        ("Linux", "ARM64", "linux-arm64"),
        ("macOS", "ARM64", "macos-arm64"),
    ] {
        let root = tempfile::tempdir()?;
        fake_runner(root.path())?;
        // Swap in the fake executable's digest for this platform's row only.
        let text = script(&ToolCatalog::pinned()).replace(pinned_digest(platform)?, &hex(FAKE_BYTES));
        let output = run(root.path(), &text, os, arch)?;
        assert!(
            output.status.success(),
            "{platform}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(&format!("({platform}) executable SHA-256")), "{stdout}");
        let log = fs::read_to_string(root.path().join("mise.log"))?;
        assert_eq!(
            log.trim(),
            format!("--no-config --no-env --no-hooks which --tool jactionlint@{JACTIONLINT_VERSION} jactionlint")
        );
    }
    Ok(())
}

#[test]
fn a_different_executable_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    fake_runner(root.path())?;
    let output = run(root.path(), &script(&ToolCatalog::pinned()), "Linux", "X64")?;
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("jactionlint_executable_digest_mismatch"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn the_catalog_pin_on_an_unknown_platform_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    fake_runner(root.path())?;
    let output = run(root.path(), &script(&ToolCatalog::pinned()), "Windows", "X64")?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(NOT_CATALOGUED_CODE));
    assert!(!root.path().join("mise.log").exists(), "mise must not run");
    Ok(())
}

#[test]
fn a_project_version_without_a_digest_keeps_mise_verification() -> Result<(), Box<dyn Error>> {
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Jactionlint, "9.9.9")?
        .for_emission();
    let text = script(&catalog);
    assert!(text.contains("v=\"${VELNOR_TOOL_JACTIONLINT}\""), "{text}");
    let root = tempfile::tempdir()?;
    fake_runner(root.path())?;
    let path = format!("{}:{}", root.path().join("bin").display(), std::env::var("PATH")?);
    let output = Command::new("sh")
        .arg("-c")
        .arg(&text)
        .env("PATH", path)
        .env("RUNNER_TEMP", root.path())
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("VELNOR_TOOL_JACTIONLINT", "9.9.9")
        .output()?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("::notice::jactionlint 9.9.9"));
    Ok(())
}

#[test]
fn the_step_is_a_scrubbed_shell_step_with_every_catalogued_digest() -> Result<(), Box<dyn Error>> {
    let step = verify_jactionlint_step(&ToolCatalog::pinned())?;
    assert_eq!(step.name, VERIFY_JACTIONLINT_STEP);
    let text = script(&ToolCatalog::pinned());
    for row in velnor_actions_mise::JACTIONLINT_DIGESTS {
        assert!(text.contains(row.executable_sha256), "{}", row.platform);
    }
    assert!(!text.contains("$("), "{text}");
    assert!(!text.contains('`'), "{text}");
    assert!(!text.contains('\n'), "{text}");
    Ok(())
}
