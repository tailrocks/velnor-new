// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult<T> = Result<T, Box<dyn Error>>;

const SOURCE: &str = "1111111111111111111111111111111111111111";
const HELPER: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REFUSAL: &str = "local mode cannot be built into a release";

fn compile_build_script(directory: &Path) -> TestResult<PathBuf> {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = directory.join("build_script");
    let status = Command::new(rustc)
        .arg("--edition=2024")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs"))
        .arg("-o")
        .arg(&output)
        .status()?;
    assert!(status.success());
    Ok(output)
}

fn run(script: &Path, directory: &Path, local_mode: bool, release: bool) -> TestResult<Output> {
    let mut command = Command::new(script);
    command
        .env_clear()
        .env("OUT_DIR", directory)
        .env("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"));
    if local_mode {
        command.env("CARGO_FEATURE_LOCAL_MODE", "1");
    }
    if release {
        command
            .env("VELNOR_RUNNER_BUILD_SOURCE_SHA", SOURCE)
            .env("VELNOR_RUNNER_BUILD_HELPER_SHA256", HELPER);
    }
    Ok(command.output()?)
}

fn refused(output: &Output) -> bool {
    !output.status.success() && String::from_utf8_lossy(&output.stderr).contains(REFUSAL)
}

#[test]
fn a_release_identity_refuses_the_local_mode_feature() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let script = compile_build_script(directory.path())?;
    let release_with_local = run(&script, directory.path(), true, true)?;
    let local_only = run(&script, directory.path(), true, false)?;
    let neither = run(&script, directory.path(), false, false)?;
    let release_only = run(&script, directory.path(), false, true)?;
    assert!(refused(&release_with_local));
    assert!(local_only.status.success());
    assert!(neither.status.success());
    assert!(!refused(&release_only));
    Ok(())
}
