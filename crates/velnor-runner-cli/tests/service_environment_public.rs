// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLIST: &str = "Library/LaunchAgents/com.tailrocks.velnor.host.plist";

fn fake_orb(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let orb = dir.join("orb");
    std::fs::write(&orb, "#!/bin/sh\n")?;
    std::fs::set_permissions(&orb, std::fs::Permissions::from_mode(0o755))?;
    Ok(orb)
}

fn install(home: &Path, path: &str, extra: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .env("HOME", home)
        .env("PATH", path)
        .arg("--state")
        .arg(home.join("state"))
        .args(["service", "install"])
        .args(extra)
        .output()
}

fn plist(home: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(home.join(PLIST))
}

#[test]
fn install_writes_orb_from_path_and_runner_next_to_the_host() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let orb = fake_orb(&home.join("bin"))?;
    let output = install(
        &home,
        &format!("/nonexistent:{}", home.join("bin").display()),
        &[],
    )?;
    assert!(output.status.success());
    let text = plist(&home)?;
    let host = PathBuf::from(env!("CARGO_BIN_EXE_velnor-host"));
    let runner = host.parent().ok_or("no parent")?.join("velnor-runner");
    assert!(text.contains(&format!(
        "<key>VELNOR_ORB</key><string>{}</string>",
        orb.display()
    )));
    assert!(text.contains(&format!(
        "<key>VELNOR_RUNNER_BINARY</key><string>{}</string>",
        runner.display()
    )));
    assert!(text.contains("<key>StandardOutPath</key>"));
    Ok(())
}

#[test]
fn install_escapes_explicit_paths_and_is_idempotent() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let orb = fake_orb(&home.join("a&b"))?;
    let orb_arg = orb.to_str().ok_or("utf8")?;
    let args = [
        "--orb",
        orb_arg,
        "--runner-binary",
        "/opt/velnor/bin/velnor-runner",
    ];
    assert!(install(&home, "/usr/bin", &args)?.status.success());
    let first = plist(&home)?;
    assert!(first.contains(&format!(
        "<key>VELNOR_ORB</key><string>{}/a&amp;b/orb</string>",
        home.display()
    )));
    assert!(
        first.contains(
            "<key>VELNOR_RUNNER_BINARY</key><string>/opt/velnor/bin/velnor-runner</string>"
        )
    );
    assert!(install(&home, "/usr/bin", &args)?.status.success());
    assert_eq!(plist(&home)?, first);
    if cfg!(target_os = "macos") {
        let lint = Command::new("plutil")
            .arg("-lint")
            .arg(home.join(PLIST))
            .output()?;
        assert!(lint.status.success(), "{lint:?}");
    }
    Ok(())
}

#[test]
fn install_refuses_when_orb_is_missing_or_not_absolute() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    for (path, extra) in [
        ("/nonexistent", vec![]),
        ("/nonexistent", vec!["--orb", "relative/orb"]),
        ("/nonexistent", vec!["--orb", "/nonexistent/orb"]),
    ] {
        let output = install(&home, path, &extra)?;
        assert!(!output.status.success(), "{extra:?}");
        assert!(!home.join(PLIST).exists(), "{extra:?}");
    }
    let orb = fake_orb(&home.join("bin"))?;
    let orb = orb.to_str().ok_or("utf8")?;
    let output = install(
        &home,
        "/nonexistent",
        &["--orb", orb, "--runner-binary", "runner"],
    )?;
    assert!(!output.status.success());
    assert!(!home.join(PLIST).exists());
    Ok(())
}
