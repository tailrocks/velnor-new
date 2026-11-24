// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use crate::io::Repo;

use super::fixture::{TestResult, setup};

#[test]
fn a_cargo_that_forwards_back_into_the_wrapper_is_refused_instead_of_looping() -> TestResult {
    let repo = Repo::empty("crates-mirror-reentry");
    let root = repo.root();
    repo.write_unstaged("ca.pem", "not used by this fixture");
    let output = setup(root, "sparse+https://mirror.example/index/");
    assert!(output.status.success(), "{output:?}");
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    let env = fs::read_to_string(root.join("env"))?;
    let config = env
        .lines()
        .find_map(|line| line.strip_prefix("VELNOR_CARGO_CONFIG="))
        .ok_or("exported Cargo config")?;
    repo.write_unstaged(
        "shim/cargo",
        "#!/bin/sh\nexec \"$VELNOR_TEST_WRAPPER\" \"$@\"\n",
    );
    fs::set_permissions(root.join("shim/cargo"), fs::Permissions::from_mode(0o755))?;
    let output = Command::new(&wrapper)
        .args(["metadata", "--offline"])
        .current_dir(root)
        .env("VELNOR_CARGO_CONFIG", config)
        .env("VELNOR_CARGO_WRAPPER", &wrapper)
        .env("VELNOR_TEST_WRAPPER", &wrapper)
        .env("CARGO_HOME", root.join("home"))
        .env(
            "PATH",
            std::env::join_paths([
                root.join("shim"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ])?,
        )
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .env_remove("VELNOR_CARGO_DEPTH")
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("forwards back into the wrapper"),
        "{stderr}"
    );
    assert!(!stderr.contains("Argument list too long"), "{stderr}");
    Ok(())
}

// Walks the process tree, which only Linux exposes; elsewhere the env counter applies.
#[cfg(target_os = "linux")]
#[test]
fn a_shim_that_unsets_the_depth_counter_is_still_refused() -> TestResult {
    let repo = Repo::empty("crates-mirror-reentry-unset");
    let root = repo.root();
    repo.write_unstaged("ca.pem", "not used by this fixture");
    let output = setup(root, "sparse+https://mirror.example/index/");
    assert!(output.status.success(), "{output:?}");
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    let env = fs::read_to_string(root.join("env"))?;
    let config = env
        .lines()
        .find_map(|line| line.strip_prefix("VELNOR_CARGO_CONFIG="))
        .ok_or("exported Cargo config")?;
    repo.write_unstaged(
        "shim/cargo",
        "#!/bin/sh\nunset VELNOR_CARGO_DEPTH\nexec \"$VELNOR_TEST_WRAPPER\" \"$@\"\n",
    );
    fs::set_permissions(root.join("shim/cargo"), fs::Permissions::from_mode(0o755))?;
    let output = Command::new(&wrapper)
        .args(["metadata", "--offline"])
        .current_dir(root)
        .env("VELNOR_CARGO_CONFIG", config)
        .env("VELNOR_CARGO_WRAPPER", &wrapper)
        .env("VELNOR_TEST_WRAPPER", &wrapper)
        .env("CARGO_HOME", root.join("home"))
        .env(
            "PATH",
            std::env::join_paths([
                root.join("shim"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ])?,
        )
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .env_remove("VELNOR_CARGO_DEPTH")
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("forwards back into the wrapper"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn a_forward_just_below_the_depth_limit_is_not_refused() -> TestResult {
    let repo = Repo::empty("crates-mirror-reentry-below");
    let root = repo.root();
    repo.write_unstaged("ca.pem", "not used by this fixture");
    let output = setup(root, "sparse+https://mirror.example/index/");
    assert!(output.status.success(), "{output:?}");
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    let env = fs::read_to_string(root.join("env"))?;
    let config = env
        .lines()
        .find_map(|line| line.strip_prefix("VELNOR_CARGO_CONFIG="))
        .ok_or("exported Cargo config")?;
    let cargo_bin = velnor_test_support::real_cargo()
        .parent()
        .ok_or("Cargo directory")?
        .to_path_buf();
    let output = Command::new(&wrapper)
        .args(["metadata", "--offline"])
        .current_dir(root)
        .env("VELNOR_CARGO_CONFIG", config)
        .env("VELNOR_CARGO_WRAPPER", &wrapper)
        .env("CARGO_HOME", root.join("home"))
        .env(
            "PATH",
            std::env::join_paths([cargo_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])?,
        )
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .env("VELNOR_CARGO_DEPTH", "63")
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("forwards back into the wrapper"),
        "{stderr}"
    );
    Ok(())
}
