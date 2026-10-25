//! Behaviour of the generated `Verify jactionlint` script under a fake Mise.

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use velnor_actions_mise::{
    JACTIONLINT_VERSION, PinnedTool, ToolCatalog, jactionlint_executable_sha256,
};
use velnor_actions_workflow_renderer::VERIFY_PATH;

use super::{NOT_CATALOGUED_CODE, script};

const FAKE_BYTES: &[u8] = b"fake jactionlint executable\n";

#[derive(Clone, Copy)]
struct RunOptions<'a> {
    os: &'a str,
    arch: &'a str,
    mise_data_dir: Option<&'a Path>,
    xdg_data_home: Option<&'a Path>,
    home: &'a Path,
    tool_version: Option<&'a str>,
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

/// A selected Setup Mise data root with an absolute fake `bin/mise`.
fn fake_runner(root: &Path, data_dir: &Path) -> Result<(), Box<dyn Error>> {
    let bin = data_dir.join("bin");
    fake_runner_bin(root, &bin)
}

fn fake_runner_bin(root: &Path, bin: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(bin)?;
    let exe = root.join("jactionlint");
    fs::write(&exe, FAKE_BYTES)?;
    let mise = bin.join("mise");
    let data_dir_log = root.join("mise-data-dir.log");
    fs::write(
        &mise,
        format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{log}\"\nprintf '%s\\n' \"$MISE_DATA_DIR\" >> \"{log}\"\nprintf '%s' \"$MISE_DATA_DIR\" > \"{data_dir_log}\"\nprintf '%s\\n' \"$PATH\" >> \"{log}\"\nprintf '%s\\n' \"{exe}\"\n",
            log = root.join("mise.log").display(),
            data_dir_log = data_dir_log.display(),
            exe = exe.display()
        ),
    )?;
    let mut permissions = fs::metadata(&mise)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&mise, permissions)?;
    Ok(())
}

fn poison_path(root: &Path) -> Result<(), Box<dyn Error>> {
    let bin = root.join("poison-bin");
    fs::create_dir_all(&bin)?;
    for name in [
        "awk",
        "bash",
        "cp",
        "cut",
        "env",
        "grep",
        "mkdir",
        "mise",
        "rm",
        "sha256sum",
        "sh",
        "shasum",
        "tr",
    ] {
        let path = bin.join(name);
        fs::write(
            &path,
            format!("#!/bin/sh\nprintf '%s\\n' '{name}' >> \"$POISON_LOG\"\nexit 97\n"),
        )?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn run(root: &Path, script: &str, options: RunOptions<'_>) -> Result<Output, Box<dyn Error>> {
    poison_path(root)?;
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(script)
        .current_dir(root)
        .env("PATH", root.join("poison-bin"))
        .env("POISON_LOG", root.join("poison.log"))
        .env("RUNNER_TEMP", root)
        .env("RUNNER_OS", options.os)
        .env("RUNNER_ARCH", options.arch)
        .env("HOME", options.home);
    if let Some(data_dir) = options.mise_data_dir {
        command.env("MISE_DATA_DIR", data_dir);
    } else {
        command.env_remove("MISE_DATA_DIR");
    }
    if let Some(xdg) = options.xdg_data_home {
        command.env("XDG_DATA_HOME", xdg);
    } else {
        command.env_remove("XDG_DATA_HOME");
    }
    if let Some(version) = options.tool_version {
        command.env("VELNOR_TOOL_JACTIONLINT", version);
    } else {
        command.env_remove("VELNOR_TOOL_JACTIONLINT");
    }
    let output = command.output()?;
    if root.join("poison.log").exists() {
        return Err(format!(
            "jactionlint verifier resolved a helper through poisoned PATH: {}",
            fs::read_to_string(root.join("poison.log"))?
        )
        .into());
    }
    Ok(output)
}

fn pinned_digest(platform: &str) -> Result<&'static str, Box<dyn Error>> {
    jactionlint_executable_sha256(JACTIONLINT_VERSION, platform)
        .ok_or_else(|| format!("no {platform} digest").into())
}

fn assert_selected_mise_directory(
    root: &Path,
    selected: &Path,
    mise_data_dir: Option<&Path>,
    xdg_data_home: Option<&Path>,
    home: &Path,
) -> Result<(), Box<dyn Error>> {
    let text =
        script(&ToolCatalog::pinned()).replace(pinned_digest("linux-x64")?, &hex(FAKE_BYTES));
    let output = run(
        root,
        &text,
        RunOptions {
            os: "Linux",
            arch: "X64",
            mise_data_dir,
            xdg_data_home,
            home,
            tool_version: None,
        },
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let selected = fs::canonicalize(selected)?;
    assert_eq!(
        fs::read(root.join("mise-data-dir.log"))?.as_slice(),
        selected.to_str().expect("selected path").as_bytes()
    );
    Ok(())
}

#[test]
fn a_matching_executable_passes_for_every_runner_platform() -> Result<(), Box<dyn Error>> {
    for (os, arch, platform) in [
        ("Linux", "X64", "linux-x64"),
        ("Linux", "ARM64", "linux-arm64"),
        ("macOS", "ARM64", "macos-arm64"),
        ("macOS", "X64", "macos-x64"),
    ] {
        let root = tempfile::tempdir()?;
        let data_dir = root.path().join("mise-data");
        fake_runner(root.path(), &data_dir)?;
        // Swap in the fake executable's digest for this platform's row only.
        let text =
            script(&ToolCatalog::pinned()).replace(pinned_digest(platform)?, &hex(FAKE_BYTES));
        let output = run(
            root.path(),
            &text,
            RunOptions {
                os,
                arch,
                mise_data_dir: Some(&data_dir),
                xdg_data_home: None,
                home: root.path(),
                tool_version: None,
            },
        )?;
        assert!(
            output.status.success(),
            "{platform}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("({platform}) executable SHA-256")),
            "{stdout}"
        );
        let log = fs::read_to_string(root.path().join("mise.log"))?;
        assert_eq!(
            log.lines().next().expect("Mise argv"),
            format!(
                "--no-config --no-env --no-hooks which --tool jactionlint@{JACTIONLINT_VERSION} jactionlint"
            )
        );
        let expected_data_dir = fs::canonicalize(&data_dir)?;
        assert_eq!(
            log.lines().nth(1),
            Some(expected_data_dir.to_str().expect("Mise path"))
        );
        assert_eq!(log.lines().nth(2), Some(VERIFY_PATH));
    }
    Ok(())
}

#[test]
fn missing_or_symlinked_setup_mise_binary_fails_closed() -> Result<(), Box<dyn Error>> {
    let script = script(&ToolCatalog::pinned());
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("mise-data");
    fs::create_dir_all(data_dir.join("bin"))?;
    let output = run(
        root.path(),
        &script,
        RunOptions {
            os: "Linux",
            arch: "X64",
            mise_data_dir: Some(&data_dir),
            xdg_data_home: None,
            home: root.path(),
            tool_version: None,
        },
    )?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("jactionlint_mise_binary_invalid"));

    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("mise-data");
    let bin = data_dir.join("bin");
    fs::create_dir_all(&bin)?;
    let real = root.path().join("real-mise");
    fs::write(&real, "#!/bin/sh\nexit 0\n")?;
    let mut permissions = fs::metadata(&real)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&real, permissions)?;
    std::os::unix::fs::symlink(&real, bin.join("mise"))?;
    let output = run(
        root.path(),
        &script,
        RunOptions {
            os: "Linux",
            arch: "X64",
            mise_data_dir: Some(&data_dir),
            xdg_data_home: None,
            home: root.path(),
            tool_version: None,
        },
    )?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("jactionlint_mise_binary_invalid"));
    Ok(())
}

#[test]
fn a_different_executable_fails_closed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("mise-data");
    fake_runner(root.path(), &data_dir)?;
    let output = run(
        root.path(),
        &script(&ToolCatalog::pinned()),
        RunOptions {
            os: "Linux",
            arch: "X64",
            mise_data_dir: Some(&data_dir),
            xdg_data_home: None,
            home: root.path(),
            tool_version: None,
        },
    )?;
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
    let data_dir = root.path().join("mise-data");
    fake_runner(root.path(), &data_dir)?;
    let output = run(
        root.path(),
        &script(&ToolCatalog::pinned()),
        RunOptions {
            os: "Windows",
            arch: "X64",
            mise_data_dir: Some(&data_dir),
            xdg_data_home: None,
            home: root.path(),
            tool_version: None,
        },
    )?;
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
    let data_dir = root.path().join("mise-data");
    fake_runner(root.path(), &data_dir)?;
    let output = run(
        root.path(),
        &text,
        RunOptions {
            os: "Linux",
            arch: "X64",
            mise_data_dir: Some(&data_dir),
            xdg_data_home: None,
            home: root.path(),
            tool_version: Some("9.9.9"),
        },
    )?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("::notice::jactionlint 9.9.9"));
    Ok(())
}

#[path = "jactionlint_mise_selector_tests.rs"]
mod mise_selector_tests;

#[path = "jactionlint_inline_output_tests.rs"]
mod inline_output_tests;
