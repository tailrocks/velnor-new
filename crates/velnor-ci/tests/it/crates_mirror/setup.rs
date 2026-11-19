// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use crate::io::Repo;

use super::fixture::{TestResult, setup, setup_command};

const REGISTRY: &str = "sparse+https://mirror.example/index/";
const EXISTING: &str = "[net]\noffline=true\n";

#[test]
fn mirror_setup_preserves_unowned_cargo_configuration() -> TestResult {
    let repo = Repo::empty("crates-mirror-owned");
    repo.write_unstaged("ca.pem", "fixture");
    for name in ["config", "config.toml"] {
        repo.write_unstaged(&format!("home/{name}"), EXISTING);
    }
    for _ in 0..2 {
        let output = setup(repo.root(), REGISTRY);
        assert!(output.status.success(), "{output:?}");
        assert_job_config(repo.root())?;
        for name in ["config", "config.toml"] {
            assert_eq!(
                fs::read_to_string(repo.root().join(format!("home/{name}")))?,
                EXISTING
            );
        }
    }
    Ok(())
}

#[test]
fn mirror_setup_without_cargo_home_leaves_home_untouched() -> TestResult {
    let repo = Repo::empty("crates-mirror-home");
    repo.write_unstaged("ca.pem", "fixture");
    repo.write_unstaged("user-home/keep", "unchanged");
    for _ in 0..2 {
        let output = setup_command(repo.root(), REGISTRY)
            .env_remove("CARGO_HOME")
            .env("HOME", repo.root().join("user-home"))
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(!repo.root().join("user-home/.cargo").exists());
        assert_eq!(
            fs::read_to_string(repo.root().join("user-home/keep"))?,
            "unchanged"
        );
        assert_job_config(repo.root())?;
    }
    Ok(())
}

#[test]
fn mirror_setup_refuses_unowned_job_configuration() -> TestResult {
    let repo = Repo::empty("crates-mirror-job-owned");
    repo.write_unstaged("ca.pem", "fixture");
    repo.write_unstaged("velnor-cargo/config.toml", EXISTING);
    let output = setup(repo.root(), REGISTRY);
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join("velnor-cargo/config.toml"))?,
        EXISTING
    );
    Ok(())
}

fn assert_job_config(root: &Path) -> TestResult {
    let env = fs::read_to_string(root.join("env"))?;
    for config in env
        .lines()
        .filter_map(|line| line.strip_prefix("VELNOR_CARGO_CONFIG="))
    {
        let config = fs::canonicalize(config)?;
        assert!(config.starts_with(fs::canonicalize(root)?));
        let text = fs::read_to_string(config)?;
        assert!(text.contains(REGISTRY));
        assert!(text.contains(&root.join("ca.pem").display().to_string()));
    }
    assert!(env.contains("VELNOR_CARGO_CONFIG="));
    Ok(())
}

#[test]
fn mirror_setup_repeats_from_a_read_only_helper() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let repo = Repo::empty("crates-mirror-read-only");
    repo.write_unstaged("ca.pem", "fixture");
    let helper = repo.root().join("velnor-ci-exec");
    fs::copy(env!("CARGO_BIN_EXE_velnor-ci"), &helper)?;
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o555))?;
    for _ in 0..2 {
        let output = std::process::Command::new(&helper)
            .args(["crates-mirror", "--registry", REGISTRY, "--cainfo"])
            .arg(repo.root().join("ca.pem"))
            .env("CARGO_HOME", repo.root().join("home"))
            .env("RUNNER_TEMP", repo.root())
            .env("GITHUB_ENV", repo.root().join("env"))
            .env("GITHUB_PATH", repo.root().join("path"))
            .output()?;
        assert!(output.status.success(), "{output:?}");
    }
    Ok(())
}
