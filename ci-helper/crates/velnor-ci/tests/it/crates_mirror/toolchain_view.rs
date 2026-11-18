// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::io::Repo;

use super::fixture::{TestResult, active_toolchain, rustup_binary, setup};

const REGISTRY: &str = "sparse+https://mirror.example/index/";
const DEFAULT_ALIAS: &str = "mirror-default";
const FAKE_ALIAS: &str = "mirror-fake";

#[test]
fn a_build_keeps_its_own_rustc_when_a_build_script_selects_another_toolchain() -> TestResult {
    rustup_binary()
        .ok_or("rustup unavailable: mirror integration requires a real Rustup binary")?;
    let repo = Repo::empty("crates-mirror-own-rustc");
    let root = repo.root();
    repo.write_unstaged("ca.pem", "not used by this fixture");
    fake_toolchain(root)?;
    let home = private_home(root)?;
    projects(&repo);
    let output = setup(root, REGISTRY);
    assert!(output.status.success(), "{output:?}");
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    let config = job_config(root)?;
    let paths = [
        root.join("proxy-bin"),
        wrapper.parent().ok_or("wrapper parent")?.to_path_buf(),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    let output = Command::new(&wrapper)
        .args(["build", "--offline", "--manifest-path"])
        .arg(root.join("consumer/Cargo.toml"))
        .current_dir(root.join("consumer"))
        .env("MIRROR_FIXTURE_ROOT", root)
        .env("RUSTUP_HOME", &home)
        .env("VELNOR_CARGO_CONFIG", &config)
        .env("VELNOR_CARGO_WRAPPER", &wrapper)
        .env("CARGO_HOME", root.join("outer-home"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("PATH", std::env::join_paths(paths)?)
        .env_remove("RUSTC")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let expected = Command::new(PathBuf::from(active_toolchain()?).join("bin/rustc"))
        .arg("-vV")
        .output()?;
    assert!(expected.status.success(), "{expected:?}");
    let expected = String::from_utf8_lossy(&expected.stdout);
    for name in ["rustc-before.txt", "rustc-after.txt"] {
        let seen = fs::read_to_string(root.join(name))?;
        assert_eq!(
            seen, expected,
            "{name} must resolve the build's own toolchain rustc"
        );
    }
    Ok(())
}

fn projects(repo: &Repo) {
    repo.write_unstaged(
        "consumer/Cargo.toml",
        "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\nbuild='build.rs'\n",
    );
    repo.write_unstaged("consumer/src/lib.rs", "");
    repo.write_unstaged(
        "tiny/Cargo.toml",
        "[package]\nname='tiny'\nversion='1.0.0'\nedition='2024'\n",
    );
    repo.write_unstaged("tiny/src/lib.rs", "");
    repo.write_unstaged(
        "consumer/build.rs",
        r#"fn main() {
    let root = std::env::var("MIRROR_FIXTURE_ROOT").unwrap();
    let record = |name: &str| {
        let rustc = std::process::Command::new("rustc").arg("-vV").output().unwrap();
        assert!(rustc.status.success());
        std::fs::write(format!("{root}/{name}"), &rustc.stdout).unwrap();
    };
    record("rustc-before.txt");
    let nested = std::process::Command::new(std::env::var("CARGO").unwrap())
        .args([
            "+mirror-fake",
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--manifest-path",
        ])
        .arg(format!("{root}/tiny/Cargo.toml"))
        .env("CARGO_HOME", format!("{root}/nested-home"))
        .output()
        .unwrap();
    assert!(nested.status.success(), "nested cargo failed: {nested:?}");
    record("rustc-after.txt");
}"#,
    );
}

fn fake_toolchain(root: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new(rustup_binary().ok_or("rustup unavailable")?)
        .args(["which", "cargo"])
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let cargo = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    let toolchain = root.join("fake-toolchain");
    let bin = toolchain.join("bin");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(toolchain.join("lib"))?;
    fs::copy(&cargo, bin.join("cargo"))?;
    fs::write(bin.join("rustc"), "#!/bin/sh\necho mirror-fake-rustc\n")?;
    fs::set_permissions(bin.join("rustc"), fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn private_home(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let installed = rustup_binary().ok_or("rustup unavailable")?;
    let bin = root.join("proxy-bin");
    fs::create_dir(&bin)?;
    fs::copy(installed, bin.join("rustup"))?;
    std::os::unix::fs::symlink(bin.join("rustup"), bin.join("cargo"))?;
    let home = root.join("rustup-home");
    for (alias, target) in [
        (DEFAULT_ALIAS, PathBuf::from(active_toolchain()?)),
        (FAKE_ALIAS, root.join("fake-toolchain")),
    ] {
        let output = rustup(&bin, &home)
            .args(["toolchain", "link", alias])
            .arg(target)
            .output()?;
        assert!(output.status.success(), "{output:?}");
    }
    let output = rustup(&bin, &home)
        .args(["default", DEFAULT_ALIAS])
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(home)
}

fn rustup(bin: &Path, home: &Path) -> Command {
    let mut command = Command::new(bin.join("rustup"));
    command
        .current_dir(bin)
        .env("RUSTUP_HOME", home)
        .env_remove("RUSTUP_TOOLCHAIN");
    command
}

fn job_config(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let env = fs::read_to_string(root.join("env"))?;
    Ok(PathBuf::from(
        env.lines()
            .find_map(|line| line.strip_prefix("VELNOR_CARGO_CONFIG="))
            .ok_or("exported Cargo config")?,
    ))
}
