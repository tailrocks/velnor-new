// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::io::Repo;

use super::fixture::{TestResult, setup, setup_command};

pub(super) const REGISTRY: &str = "sparse+https://mirror.example/index/";
pub(super) const MBX_DECLARED: &str = "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\n\n[wrappers.cargo]\ncommand = \"mbx\"\n";
pub(super) const REFUSED: &str = "mbx is declared but cargo on PATH isn't the mise mbx wrapper";

/// A `cargo` that prints a marker, standing in for a cargo found on PATH outside mise.
pub(super) fn plain_cargo(repo: &Repo) -> Result<PathBuf, Box<dyn std::error::Error>> {
    script(repo, "plain/cargo", "#!/bin/sh\necho plain-cargo-ran\n")?;
    Ok(repo.root().join("plain"))
}

/// A directory whose `cargo` is a symlink to a `mise` stand-in, as mise's command-wrappers are.
fn mise_wrapper(repo: &Repo) -> Result<PathBuf, Box<dyn std::error::Error>> {
    script(repo, "mise-bin/mise", "#!/bin/sh\necho fake-mise \"$@\"\n")?;
    std::os::unix::fs::symlink(
        repo.root().join("mise-bin/mise"),
        repo.root().join("mise-bin/cargo"),
    )?;
    Ok(repo.root().join("mise-bin"))
}

fn script(repo: &Repo, path: &str, text: &str) -> TestResult {
    repo.write_unstaged(path, text);
    fs::set_permissions(repo.root().join(path), fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(super) fn path_with(first: &Path) -> Result<std::ffi::OsString, Box<dyn std::error::Error>> {
    Ok(std::env::join_paths([
        first.to_path_buf(),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ])?)
}

/// Runs the configured wrapper the way a `cargo` call from a build tool reaches it.
fn run_wrapper(
    root: &Path,
    first: &Path,
    mbx_required: bool,
) -> Result<Output, Box<dyn std::error::Error>> {
    run_wrapper_with(root, first, mbx_required, false)
}

/// `mise_active` models a step whose environment already has mise's cargo wrapper active
/// (`__MISE_ACTIVE_WRAPPERS` lists cargo) but not the velnor launch phase: the first `cargo`
/// the step reaches is the wrapper itself, outside mise's own `exec` chain.
fn run_wrapper_with(
    root: &Path,
    first: &Path,
    mbx_required: bool,
    mise_active: bool,
) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(wrapper_command(root, first, mbx_required, mise_active)?.output()?)
}

pub(super) fn wrapper_command(
    root: &Path,
    first: &Path,
    mbx_required: bool,
    mise_active: bool,
) -> Result<Command, Box<dyn std::error::Error>> {
    let env = fs::read_to_string(root.join("env"))?;
    let config = env
        .lines()
        .find_map(|line| line.strip_prefix("VELNOR_CARGO_CONFIG="))
        .map(str::to_owned);
    let mirror_off = env.lines().any(|line| line == "VELNOR_CRATES_MIRROR=off");
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    let mut command = Command::new(&wrapper);
    command
        .args(["--version"])
        .current_dir(root)
        .env("VELNOR_CARGO_WRAPPER", &wrapper)
        .env("CARGO_HOME", root.join("home"))
        .env("MISE_DATA_DIR", root.join("mise-data"))
        .env("PATH", path_with(first)?)
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .env_remove("VELNOR_CARGO_CONFIG")
        .env_remove("VELNOR_CRATES_MIRROR")
        .env_remove("VELNOR_CARGO_DEPTH")
        .env_remove("VELNOR_CARGO_MISE_PHASE")
        .env_remove("__MISE_ACTIVE_WRAPPERS");
    if let Some(config) = config {
        command.env("VELNOR_CARGO_CONFIG", config);
    }
    if mirror_off {
        command.env("VELNOR_CRATES_MIRROR", "off");
    }
    if mbx_required {
        command.env("VELNOR_CARGO_MBX", "required");
    } else {
        command.env_remove("VELNOR_CARGO_MBX");
    }
    if mise_active {
        command.env("__MISE_ACTIVE_WRAPPERS", r#"["cargo"]"#);
    }
    Ok(command)
}

pub(super) fn configured_repo(name: &str) -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = Repo::empty(name);
    repo.write_unstaged("ca.pem", "not used by this fixture");
    fs::create_dir_all(repo.root().join("mise-data"))?;
    let output = setup(repo.root(), REGISTRY);
    assert!(output.status.success(), "{output:?}");
    Ok(repo)
}

#[test]
fn declared_mbx_refuses_a_plain_cargo_ahead_of_the_mise_wrapper() -> TestResult {
    let repo = configured_repo("crates-mirror-mbx-plain")?;
    let plain = plain_cargo(&repo)?;
    let output = run_wrapper(repo.root(), &plain, true)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains(REFUSED), "{stderr}");
    assert!(stderr.contains(&plain.display().to_string()), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    Ok(())
}

#[test]
fn declared_mbx_refuses_a_first_hop_cargo_in_an_active_mise_environment() -> TestResult {
    // Reached first-hop (no launch phase, no pinned cargo): the native-ready arm must not run
    // a plain cargo around mise's MBX wrapper.
    let repo = configured_repo("crates-mirror-mbx-ready-native")?;
    let plain = plain_cargo(&repo)?;
    let output = run_wrapper_with(repo.root(), &plain, true, true)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains(REFUSED), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    Ok(())
}

#[test]
fn undeclared_mbx_keeps_running_a_plain_cargo_in_an_active_mise_environment() -> TestResult {
    let repo = configured_repo("crates-mirror-mbx-ready-native-undeclared")?;
    let plain = plain_cargo(&repo)?;
    let output = run_wrapper_with(repo.root(), &plain, false, true)?;
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    Ok(())
}

#[test]
fn declared_mbx_runs_the_mise_wrapper_when_it_is_first_on_path() -> TestResult {
    let repo = configured_repo("crates-mirror-mbx-mise")?;
    let mise = mise_wrapper(&repo)?;
    let output = run_wrapper(repo.root(), &mise, true)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("fake-mise exec --"), "{stdout}");
    Ok(())
}

#[test]
fn undeclared_mbx_keeps_running_a_plain_cargo() -> TestResult {
    let repo = configured_repo("crates-mirror-mbx-undeclared")?;
    let plain = plain_cargo(&repo)?;
    let output = run_wrapper(repo.root(), &plain, false)?;
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    Ok(())
}

#[test]
fn setup_records_a_declared_mbx_for_every_forward() -> TestResult {
    for file in ["mise.toml", ".mise.toml"] {
        let repo = Repo::empty("crates-mirror-mbx-record");
        repo.write_unstaged("ca.pem", "not used by this fixture");
        repo.write_unstaged(file, MBX_DECLARED);
        let output = setup_command(repo.root(), REGISTRY)
            .current_dir(repo.root())
            .output()?;
        assert!(output.status.success(), "{file}: {output:?}");
        let env = fs::read_to_string(repo.root().join("env"))?;
        assert!(
            env.lines().any(|line| line == "VELNOR_CARGO_MBX=required"),
            "{file}: {env}"
        );
    }
    Ok(())
}

#[test]
fn setup_does_not_record_mbx_when_nothing_declares_it() -> TestResult {
    let repo = Repo::empty("crates-mirror-mbx-none");
    repo.write_unstaged("ca.pem", "not used by this fixture");
    repo.write_unstaged("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    let output = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let env = fs::read_to_string(repo.root().join("env"))?;
    assert!(!env.contains("VELNOR_CARGO_MBX"), "{env}");
    Ok(())
}

#[test]
fn mirror_off_with_declared_mbx_and_no_cargo_on_path_succeeds() -> TestResult {
    // The crates-mirror step runs before the tools install, so mise's cargo wrapper does not
    // exist yet. The check runs at use time instead, in the forwarded cargo.
    let repo = Repo::empty("crates-mirror-mbx-off-none");
    repo.write_unstaged("mise.toml", MBX_DECLARED);
    let output = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .env("VELNOR_CRATES_MIRROR", "off")
        .env("PATH", path_with(repo.root())?)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let env = fs::read_to_string(repo.root().join("env"))?;
    let wrapper = fs::canonicalize(repo.root().join("velnor-cargo/cargo"))?;
    assert!(
        env.lines()
            .any(|line| line == format!("CARGO={}", wrapper.display())),
        "{env}"
    );
    assert!(
        env.lines().any(|line| line == "VELNOR_CARGO_MBX=required"),
        "{env}"
    );
    assert!(!env.contains("VELNOR_CARGO_CONFIG"), "{env}");
    assert!(
        env.lines().any(|line| line == "VELNOR_CRATES_MIRROR=off"),
        "{env}"
    );
    assert!(!repo.root().join("velnor-cargo/config.toml").exists());
    Ok(())
}

#[test]
fn mirror_job_whose_config_variable_is_cleared_refuses_to_run_cargo() -> TestResult {
    let repo = configured_repo("crates-mirror-config-cleared")?;
    let env_path = repo.root().join("env");
    let env = fs::read_to_string(&env_path)?;
    let mut kept = String::new();
    for line in env
        .lines()
        .filter(|line| !line.starts_with("VELNOR_CARGO_CONFIG="))
    {
        kept.push_str(line);
        kept.push('\n');
    }
    fs::write(&env_path, kept)?;
    let plain = plain_cargo(&repo)?;
    let output = run_wrapper_with(repo.root(), &plain, false, false)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("missing VELNOR_CARGO_CONFIG"), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    Ok(())
}

#[test]
fn mirror_off_with_declared_mbx_refuses_a_plain_cargo_at_use_time() -> TestResult {
    let repo = Repo::empty("crates-mirror-mbx-off-plain");
    repo.write_unstaged("mise.toml", MBX_DECLARED);
    let output = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .env("VELNOR_CRATES_MIRROR", "off")
        .env("PATH", path_with(repo.root())?)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let plain = plain_cargo(&repo)?;
    let output = run_wrapper_with(repo.root(), &plain, true, false)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains(REFUSED), "{stderr}");
    assert!(stderr.contains(&plain.display().to_string()), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    Ok(())
}

#[test]
fn mirror_off_with_declared_mbx_routes_cargo_through_the_forward_shim() -> TestResult {
    let repo = Repo::empty("crates-mirror-mbx-off-mise");
    repo.write_unstaged("mise.toml", MBX_DECLARED);
    let mise = mise_wrapper(&repo)?;
    let output = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .env("VELNOR_CRATES_MIRROR", "off")
        .env("PATH", path_with(&mise)?)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let env = fs::read_to_string(repo.root().join("env"))?;
    let wrapper = fs::canonicalize(repo.root().join("velnor-cargo/cargo"))?;
    assert!(
        env.lines()
            .any(|line| line == format!("CARGO={}", wrapper.display())),
        "{env}"
    );
    assert!(
        env.lines()
            .any(|line| line == format!("VELNOR_CARGO_WRAPPER={}", wrapper.display())),
        "{env}"
    );
    assert!(!repo.root().join("velnor-cargo/config.toml").exists());
    Ok(())
}
