// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Output;

use crate::io::Repo;

use super::super::fixture::{TestResult, setup_command};
use super::{MBX_DECLARED, REGISTRY, configured_repo, plain_cargo, run_wrapper_with};

const CONFIG_FILES: [&str; 6] = [
    "mise.toml",
    ".mise.toml",
    "mise/config.toml",
    ".mise/config.toml",
    ".config/mise.toml",
    ".config/mise/config.toml",
];
const ARRAY_FORM: &str = "[tools]\nrust = [{ version = \"1.99.1\", mr_boxington = true }]\n";
const WRAPPER_PATH: &str = "[wrappers.cargo]\ncommand = \"/opt/tools/bin/mbx\"\n";

fn final_env(root: &Path, name: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let env = fs::read_to_string(root.join("env"))?;
    let prefix = format!("{name}=");
    Ok(env
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(&prefix).map(str::to_owned)))
}

#[test]
fn setup_records_a_declared_mbx_from_every_mise_location_and_form() -> TestResult {
    let declarations = CONFIG_FILES
        .iter()
        .map(|file| (*file, MBX_DECLARED))
        .chain([("mise.toml", ARRAY_FORM), ("mise.toml", WRAPPER_PATH)]);
    for (file, text) in declarations {
        let repo = Repo::empty("crates-mirror-mbx-record");
        repo.write_unstaged("ca.pem", "not used by this fixture");
        repo.write_unstaged(file, text);
        let output = setup_command(repo.root(), REGISTRY)
            .current_dir(repo.root())
            .output()?;
        assert!(output.status.success(), "{file}: {output:?}");
        assert_eq!(
            final_env(repo.root(), "VELNOR_CARGO_MBX")?.as_deref(),
            Some("required"),
            "{file}: {text}"
        );
    }
    Ok(())
}

#[test]
fn setup_run_without_mbx_or_mirror_clears_the_modes_an_earlier_run_exported() -> TestResult {
    let repo = Repo::empty("crates-mirror-mbx-stale");
    repo.write_unstaged("ca.pem", "not used by this fixture");
    repo.write_unstaged("mise.toml", MBX_DECLARED);
    let first = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .output()?;
    assert!(first.status.success(), "{first:?}");
    assert_eq!(
        final_env(repo.root(), "VELNOR_CARGO_MBX")?.as_deref(),
        Some("required")
    );
    assert!(final_env(repo.root(), "VELNOR_CARGO_CONFIG")?.is_some_and(|value| !value.is_empty()));
    repo.write_unstaged("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    let second = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .env("VELNOR_CRATES_MIRROR", "off")
        .output()?;
    assert!(second.status.success(), "{second:?}");
    assert_eq!(
        final_env(repo.root(), "VELNOR_CARGO_MBX")?.as_deref(),
        Some("")
    );
    assert_eq!(
        final_env(repo.root(), "VELNOR_CARGO_CONFIG")?.as_deref(),
        Some("")
    );
    Ok(())
}

#[test]
fn setup_run_with_the_mirror_and_no_mbx_clears_a_stale_mbx_mode() -> TestResult {
    let repo = Repo::empty("crates-mirror-mbx-stale-on");
    repo.write_unstaged("ca.pem", "not used by this fixture");
    repo.write_unstaged("mise.toml", MBX_DECLARED);
    let first = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .output()?;
    assert!(first.status.success(), "{first:?}");
    repo.write_unstaged("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    let second = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .output()?;
    assert!(second.status.success(), "{second:?}");
    assert_eq!(
        final_env(repo.root(), "VELNOR_CARGO_MBX")?.as_deref(),
        Some("")
    );
    assert!(final_env(repo.root(), "VELNOR_CARGO_CONFIG")?.is_some_and(|value| !value.is_empty()));
    Ok(())
}

fn activated_without_wrapper(mbx_required: bool) -> Result<Output, Box<dyn std::error::Error>> {
    let repo = configured_repo("crates-mirror-mbx-activated")?;
    let plain = plain_cargo(&repo)?;
    let data = repo.root().join("activated-data");
    fs::create_dir_all(&data)?;
    let data = data.display().to_string();
    run_wrapper_with(
        repo.root(),
        &plain,
        mbx_required,
        &[
            ("VELNOR_CARGO_MISE_PHASE", "activated"),
            ("VELNOR_CARGO_MISE_DATA", &data),
        ],
    )
}

#[test]
fn declared_mbx_refuses_to_continue_when_mise_never_created_its_wrapper() -> TestResult {
    let output = activated_without_wrapper(true)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr.contains("mise's cargo wrapper is missing after activation"),
        "{stderr}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    Ok(())
}

#[test]
fn undeclared_mbx_continues_with_plain_cargo_when_mise_never_created_its_wrapper() -> TestResult {
    let output = activated_without_wrapper(false)?;
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    Ok(())
}
