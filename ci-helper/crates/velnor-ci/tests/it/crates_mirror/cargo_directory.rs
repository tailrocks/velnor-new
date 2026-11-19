// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::io::Repo;

use super::fixture::{TestResult, active_toolchain, rustup_binary, setup};

const REGISTRY: &str = "sparse+https://mirror.example/index/";

fn recorded_build() -> Result<(Repo, PathBuf), Box<dyn Error>> {
    rustup_binary()
        .ok_or("rustup unavailable: mirror integration requires a real Rustup binary")?;
    let repo = Repo::empty("crates-mirror-cargo-directory");
    let root = repo.root();
    repo.write_unstaged("ca.pem", "not used by this fixture");
    repo.write_unstaged(
        "consumer/Cargo.toml",
        "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\nbuild='build.rs'\n",
    );
    repo.write_unstaged("consumer/src/lib.rs", "");
    repo.write_unstaged(
        "consumer/build.rs",
        r#"fn main() {
    let cargo = std::env::var("CARGO").unwrap();
    std::fs::write(std::env::var("MIRROR_CARGO_RECORD").unwrap(), cargo).unwrap();
}"#,
    );
    let output = setup(root, REGISTRY);
    assert!(output.status.success(), "{output:?}");
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    let config = job_config(root)?;
    let toolchain = PathBuf::from(active_toolchain()?).join("bin");
    let output = Command::new(&wrapper)
        .args(["build", "--offline", "--manifest-path"])
        .arg(root.join("consumer/Cargo.toml"))
        .current_dir(root.join("consumer"))
        .env("MIRROR_CARGO_RECORD", root.join("recorded-cargo.txt"))
        .env("VELNOR_CARGO_CONFIG", config)
        .env("VELNOR_CARGO_WRAPPER", &wrapper)
        .env("CARGO_HOME", root.join("home"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env(
            "PATH",
            std::env::join_paths([toolchain, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])?,
        )
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let recorded = PathBuf::from(fs::read_to_string(root.join("recorded-cargo.txt"))?);
    let mirror = fs::canonicalize(root.join("velnor-cargo"))?;
    assert!(
        recorded.starts_with(mirror),
        "the build must get its Cargo from the mirror, got {}",
        recorded.display()
    );
    Ok((repo, recorded))
}

fn job_config(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let env = fs::read_to_string(root.join("env"))?;
    Ok(PathBuf::from(
        env.lines()
            .find_map(|line| line.strip_prefix("VELNOR_CARGO_CONFIG="))
            .ok_or("exported Cargo config")?,
    ))
}

#[test]
fn rustc_sits_next_to_the_cargo_a_build_script_is_given() -> TestResult {
    let (_repo, recorded) = recorded_build()?;
    let directory = recorded.parent().ok_or("cargo executable has no parent")?;
    let output = Command::new(directory.join("rustc"))
        .args(["-vV"])
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let expected = Command::new(PathBuf::from(active_toolchain()?).join("bin/rustc"))
        .args(["-vV"])
        .output()?;
    assert!(expected.status.success(), "{expected:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&expected.stdout)
    );
    Ok(())
}

#[test]
fn no_toolchain_binary_is_shared_between_builds_through_the_wrapper_directory() -> TestResult {
    let (repo, _recorded) = recorded_build()?;
    let shared = repo.root().join("velnor-cargo");
    assert!(
        !shared.join("rustc").exists(),
        "a rustc link in the shared wrapper directory is repointed by every toolchain"
    );
    Ok(())
}

#[test]
fn a_path_rebuilt_from_the_cargo_directory_still_finds_cargo() -> TestResult {
    let (repo, recorded) = recorded_build()?;
    let root = repo.root();
    let directory = recorded.parent().ok_or("cargo executable has no parent")?;
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    let output = Command::new(&recorded)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--manifest-path",
        ])
        .arg(root.join("consumer/Cargo.toml"))
        .current_dir(root.join("consumer"))
        .env(
            "PATH",
            std::env::join_paths([
                directory.to_path_buf(),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ])?,
        )
        .env("VELNOR_CARGO_CONFIG", job_config(root)?)
        .env("VELNOR_CARGO_WRAPPER", wrapper)
        .env("CARGO_HOME", root.join("home"))
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"name\":\"consumer\""), "{stdout}");
    Ok(())
}
