// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "service_environment_public/plist_tree.rs"]
mod plist_tree;

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLIST: &str = "Library/LaunchAgents/com.tailrocks.velnor.host.plist";
const RECORD: &str = "state/service-environment.json";

fn executable(path: &Path, mode: u32) -> std::io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

fn host_copy(dir: &Path, with_runner: bool) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let host = dir.join("velnor-host");
    std::fs::copy(env!("CARGO_BIN_EXE_velnor-host"), &host)?;
    if with_runner {
        let runner = dir.join("velnor-runner");
        std::fs::write(&runner, "#!/bin/sh\n")?;
        executable(&runner, 0o755)?;
    }
    Ok(host)
}

fn fake_tool(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let tool = dir.join(name);
    std::fs::write(&tool, "#!/bin/sh\n")?;
    executable(&tool, 0o755)?;
    Ok(tool)
}

fn fake_orb(dir: &Path) -> std::io::Result<PathBuf> {
    fake_tool(dir, "orb")
}

fn fake_runner(dir: &Path) -> std::io::Result<String> {
    Ok(fake_tool(dir, "velnor-runner")?
        .to_str()
        .ok_or(std::io::ErrorKind::InvalidData)?
        .to_owned())
}

fn install(home: &Path, path: &str, extra: &[&str]) -> std::io::Result<Output> {
    install_with(
        env!("CARGO_BIN_EXE_velnor-host").as_ref(),
        home,
        path,
        extra,
    )
}

fn install_with(host: &Path, home: &Path, path: &str, extra: &[&str]) -> std::io::Result<Output> {
    Command::new(host)
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
    let host = host_copy(&home.join("host"), true)?;
    let output = install_with(
        &host,
        &home,
        &format!("/nonexistent:{}", home.join("bin").display()),
        &[],
    )?;
    assert!(output.status.success(), "{output:?}");
    let tree = plist_tree::parse(&plist(&home)?).ok_or("the plist does not parse")?;
    let mut keys = tree.keys();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "EnvironmentVariables",
            "Label",
            "ProgramArguments",
            "RunAtLoad",
            "StandardErrorPath",
            "StandardOutPath"
        ]
    );
    let environment = tree.get("EnvironmentVariables").ok_or("no environment")?;
    assert_eq!(environment.keys(), ["VELNOR_ORB", "VELNOR_RUNNER_BINARY"]);
    let value = |key: &str| environment.get(key).and_then(plist_tree::Value::text);
    assert_eq!(value("VELNOR_ORB"), orb.to_str());
    assert_eq!(
        value("VELNOR_RUNNER_BINARY"),
        home.join("host/velnor-runner").to_str()
    );
    let logs = home.join("Library/Logs/Velnor");
    let log = |key: &str| tree.get(key).and_then(plist_tree::Value::text);
    assert_eq!(log("StandardOutPath"), logs.join("host.log").to_str());
    assert_eq!(log("StandardErrorPath"), logs.join("host.err.log").to_str());
    Ok(())
}

#[test]
fn install_escapes_explicit_paths_and_is_idempotent() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let orb = fake_orb(&home.join("a&b"))?;
    let orb_arg = orb.to_str().ok_or("utf8")?;
    let runner = fake_runner(&home.join("opt&r"))?;
    let args = ["--orb", orb_arg, "--runner-binary", runner.as_str()];
    assert!(install(&home, "/usr/bin", &args)?.status.success());
    let first = plist(&home)?;
    assert!(first.contains(&format!(
        "<key>VELNOR_ORB</key><string>{}/a&amp;b/orb</string>",
        home.display()
    )));
    assert!(first.contains(&format!(
        "<key>VELNOR_RUNNER_BINARY</key><string>{}/opt&amp;r/velnor-runner</string>",
        home.display()
    )));
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
    let runner = fake_runner(&home.join("opt"))?;
    for (path, extra) in [
        ("/nonexistent", vec![]),
        ("/nonexistent", vec!["--orb", "relative/orb"]),
        ("/nonexistent", vec!["--orb", "/nonexistent/orb"]),
    ] {
        let extra = [extra, vec!["--runner-binary", runner.as_str()]].concat();
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

#[test]
fn install_refuses_a_default_runner_that_is_missing_or_not_executable() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let orb = fake_orb(&home.join("bin"))?;
    let orb = orb.to_str().ok_or("utf8")?;
    let host = host_copy(&home.join("host"), false)?;
    let output = install_with(&host, &home, "/nonexistent", &["--orb", orb])?;
    assert!(!output.status.success());
    assert!(!home.join(PLIST).exists());
    let runner = home.join("host/velnor-runner");
    std::fs::write(&runner, "")?;
    executable(&runner, 0o644)?;
    let output = install_with(&host, &home, "/nonexistent", &["--orb", orb])?;
    assert!(!output.status.success());
    assert!(!home.join(PLIST).exists());
    Ok(())
}

#[test]
fn install_refuses_an_explicit_runner_that_is_missing_or_not_executable()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let orb = fake_orb(&home.join("bin"))?;
    let orb = orb.to_str().ok_or("utf8")?;
    let missing = home.join("opt/velnor-runner");
    let plain = fake_runner(&home.join("plain"))?;
    executable(plain.as_ref(), 0o644)?;
    for runner in [missing.to_str().ok_or("utf8")?, plain.as_str()] {
        let output = install(
            &home,
            "/nonexistent",
            &["--orb", orb, "--runner-binary", runner],
        )?;
        assert!(!output.status.success(), "{runner}");
        assert!(!home.join(PLIST).exists(), "{runner}");
        assert!(!home.join(RECORD).exists(), "{runner}");
    }
    Ok(())
}

#[test]
fn install_refuses_a_control_character_in_the_orb_path() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let orb = fake_orb(&home.join("a\u{b}b"))?;
    let orb = orb.to_str().ok_or("utf8")?;
    let runner = fake_runner(&home.join("opt"))?;
    let output = install(
        &home,
        "/nonexistent",
        &["--orb", orb, "--runner-binary", runner.as_str()],
    )?;
    assert!(!output.status.success());
    assert!(!home.join(PLIST).exists());
    assert!(!home.join(RECORD).exists());
    Ok(())
}

#[test]
fn start_asks_to_install_again_when_the_environment_was_never_recorded()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let orb = fake_orb(&home.join("bin"))?;
    let orb = orb.to_str().ok_or("utf8")?;
    let runner = fake_runner(&home.join("opt"))?;
    let args = ["--orb", orb, "--runner-binary", runner.as_str()];
    assert!(install(&home, "/nonexistent", &args)?.status.success());
    assert!(home.join(RECORD).exists());
    std::fs::remove_file(home.join(RECORD))?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .env("HOME", &home)
        .arg("--state")
        .arg(home.join("state"))
        .args(["service", "start"])
        .output()?;
    assert!(!output.status.success());
    let message = String::from_utf8(output.stderr)?;
    assert!(
        message.contains("`velnor-host service install`"),
        "{message}"
    );
    Ok(())
}
