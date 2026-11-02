// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PHASE: &str = "build-source-cli-and-archive-guard-concurrently";
const VERSION_STUB: &str = "#!/bin/sh\nprintf 'velnor-actions 1.2.3\\n'\n";
const DIGEST_STUB: &str = "#!/bin/sh\nprintf '%064d  -\\n' 7\n";
const FINGERPRINT_STUB: &str = "#!/bin/sh\nprintf '%064d\\n' 9\n";
const STAT_STUB: &str = "#!/bin/sh\ncase \"$2\" in\n  %s) printf '100\\n' ;;\n  *) printf '%s:755:1:100\\n' \"$(id -u)\" ;;\nesac\n";
const MISE_STUB: &str = "#!/bin/sh\ncase \" $* \" in\n  *\" config get \"*) printf '1.2.3\\n' ;;\n  *\" exec \"*) printf 'cli build output\\n'; [ \"${STUB_CLI_BUILD_FAILS:-0}\" = 0 ] ;;\nesac\n";
const GUARD_BUILDER_STUB: &str = "#!/bin/sh\nsleep 1\nprintf 'guard build output\\n'\n[ \"${STUB_GUARD_BUILD_FAILS:-0}\" = 0 ] || exit 1\nmkdir -p .velnor/archive-guard/bin\ncp \"$STUB_GUARD_BINARY\" .velnor/archive-guard/bin/velnor-archive-guard\nchmod 755 .velnor/archive-guard/bin/velnor-archive-guard\n";

struct Fixture {
    _root: tempfile::TempDir,
    runner_temp: PathBuf,
    workspace: PathBuf,
    stubs: PathBuf,
    target: PathBuf,
}

fn write_executable(path: &Path, content: &str) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let base = fs::canonicalize(root.path())?;
    let fixture = Fixture {
        runner_temp: base.join("runner-temp"),
        workspace: base.join("workspace"),
        stubs: base.join("stubs"),
        target: base.join("target"),
        _root: root,
    };
    fs::create_dir_all(evidence(&fixture).join("logs"))?;
    write_executable(&fixture.stubs.join("mise"), MISE_STUB)?;
    write_executable(&fixture.stubs.join("sha256sum"), DIGEST_STUB)?;
    write_executable(&fixture.stubs.join("stat"), STAT_STUB)?;
    write_executable(&fixture.stubs.join("guard-binary"), FINGERPRINT_STUB)?;
    write_executable(
        &fixture
            .workspace
            .join("scripts/build-owned-archive-guard.sh"),
        GUARD_BUILDER_STUB,
    )?;
    write_executable(&fixture.target.join("release/velnor-actions"), VERSION_STUB)?;
    write_executable(
        &fixture
            .target
            .join("release/examples/qualification-diagnostics"),
        VERSION_STUB,
    )?;
    Ok(fixture)
}

fn evidence(fixture: &Fixture) -> PathBuf {
    fixture.runner_temp.join("source-qualification-artifacts")
}

fn run_build_phase(
    fixture: &Fixture,
    cli_fails: bool,
    guard_fails: bool,
) -> Result<Output, Box<dyn Error>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/source-qualification.sh");
    let path = format!(
        "{}:{}",
        fixture.stubs.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Ok(Command::new("bash")
        .arg(script)
        .arg(PHASE)
        .current_dir(&fixture.workspace)
        .env("PATH", path)
        .env("RUNNER_TEMP", &fixture.runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("GITHUB_WORKSPACE", &fixture.workspace)
        .env("GITHUB_SHA", "0".repeat(40))
        .env("GITHUB_ENV", fixture.runner_temp.join("github-env"))
        .env("CARGO_TARGET_DIR", &fixture.target)
        .env("VELNOR_RUST_VERSION", "1.0.0")
        .env("VELNOR_MBX_VERSION", "1.0.0")
        .env("VELNOR_MISE_VERSION", "1.0.0")
        .env("VELNOR_NEXTEST_VERSION", "1.0.0")
        .env("STUB_GUARD_BINARY", fixture.stubs.join("guard-binary"))
        .env("STUB_CLI_BUILD_FAILS", if cli_fails { "1" } else { "0" })
        .env(
            "STUB_GUARD_BUILD_FAILS",
            if guard_fails { "1" } else { "0" },
        )
        .output()?)
}

fn receipt(fixture: &Fixture, name: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(
        evidence(fixture).join(format!("logs/{name}.exit")),
    )?)
}

#[test]
fn build_phase_succeeds_when_both_builds_succeed() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let output = run_build_phase(&fixture, false, false)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(receipt(&fixture, "cli-build")?, "0\n");
    assert_eq!(receipt(&fixture, "archive-guard-build")?, "0\n");
    Ok(())
}

#[test]
fn cli_build_failure_still_waits_for_the_guard_and_records_both_receipts()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let output = run_build_phase(&fixture, true, false)?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(receipt(&fixture, "cli-build")?, "1\n");
    assert_eq!(receipt(&fixture, "archive-guard-build")?, "0\n");
    assert!(String::from_utf8_lossy(&output.stdout).contains("guard build output"));
    Ok(())
}

#[test]
fn guard_build_failure_still_prints_its_console_and_records_both_receipts()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let output = run_build_phase(&fixture, false, true)?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(receipt(&fixture, "cli-build")?, "0\n");
    assert_eq!(receipt(&fixture, "archive-guard-build")?, "1\n");
    assert!(String::from_utf8_lossy(&output.stdout).contains("guard build output"));
    Ok(())
}
