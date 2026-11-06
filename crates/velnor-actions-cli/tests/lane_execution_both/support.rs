// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use velnor_actions_contract::SUPPORTED_TARGETS;
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context};

pub(super) fn qualified_tool_root() -> Result<(Option<tempfile::TempDir>, PathBuf), Box<dyn Error>>
{
    if let Some(root) = std::env::var_os("VELNOR_TEST_QUALIFIED_TOOL_ROOT") {
        return Ok((None, PathBuf::from(root).canonicalize()?));
    }
    let mise = PathBuf::from(velnor_actions_mise::command::resolve_check_program("mise")?);
    let expected = qualified_binary_digests()?
        .into_iter()
        .find(|(path, _)| *path == "bin/mise")
        .ok_or("missing mise pin")?
        .1;
    if sha(&fs::read(&mise)?) != expected {
        return Err("mise does not match the Linux pin; set VELNOR_TEST_QUALIFIED_TOOL_ROOT to a qualified installation".into());
    }
    let cargo = required_directory("CARGO_HOME")?;
    let rustup = required_directory("RUSTUP_HOME")?;
    let data = mise_data_directory()?;
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let mbx = mise_tool(
        &mise,
        "mbx",
        &format!(
            "mr-boxington@{}",
            catalog.version(velnor_actions_mise::PinnedTool::MrBoxington)
        ),
    )?;
    let nextest = mise_tool(
        &mise,
        "cargo-nextest",
        &format!(
            "aqua:nextest-rs/nextest/cargo-nextest@{}",
            catalog.version(velnor_actions_mise::PinnedTool::Nextest)
        ),
    )?;
    let staging = tempfile::tempdir()?;
    let root = staging.path().to_path_buf();
    fs::create_dir(root.join("bin"))?;
    for (source, destination) in [
        (&mise, "bin/mise"),
        (&mbx, "bin/mbx"),
        (&nextest, "bin/cargo-nextest"),
        (&cargo, "cargo"),
        (&rustup, "rustup"),
        (&data, "mise-data"),
    ] {
        link_dir(source, &root.join(destination))?;
    }
    Ok((Some(staging), root))
}

fn required_directory(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let value = std::env::var_os(name).filter(|value| !value.is_empty()).ok_or_else(|| format!("{name} is required; set VELNOR_TEST_QUALIFIED_TOOL_ROOT to a qualified installation"))?;
    Ok(PathBuf::from(value).canonicalize()?)
}

fn mise_data_directory() -> Result<PathBuf, Box<dyn Error>> {
    if std::env::var_os("MISE_DATA_DIR").is_some_and(|value| !value.is_empty()) {
        return required_directory("MISE_DATA_DIR");
    }
    let data =
        if let Some(value) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
            PathBuf::from(value).join("mise")
        } else {
            PathBuf::from(
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .ok_or("mise data location unavailable")?,
            )
            .join(".local/share/mise")
        };
    Ok(data.canonicalize()?)
}

fn mise_tool(mise: &Path, executable: &str, tool: &str) -> Result<PathBuf, Box<dyn Error>> {
    use std::os::unix::ffi::OsStringExt;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("repository root missing")?;
    let output = Command::new(mise)
        .current_dir(repository)
        .args(["which", executable, "--tool", tool])
        .env("MISE_OFFLINE", "1")
        .env("MISE_AUTO_INSTALL", "0")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "qualified tool {tool} unavailable; set VELNOR_TEST_QUALIFIED_TOOL_ROOT: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let mut bytes = output.stdout;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&b'\n') || bytes.contains(&0) {
        return Err("invalid mise which output".into());
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)).canonicalize()?)
}

pub(super) fn qualified_binary_digests() -> Result<Vec<(&'static str, String)>, Box<dyn Error>> {
    use velnor_actions_contract::config::{CheckPlatform, VelnorConfig};
    use velnor_actions_mise::catalog::source_build_bootstrap::{
        SourceBuildBootstrapHost, SourceBuildBootstrapTool, official,
    };
    let config: VelnorConfig = toml::from_str(include_str!("../../../../.velnor/config.toml"))?;
    let nextest = config
        .qualified_tools
        .iter()
        .find(|tool| tool.id == "cargo-nextest")
        .and_then(|tool| {
            tool.platforms
                .iter()
                .find(|platform| platform.platform == CheckPlatform::LinuxX64)
        })
        .and_then(|platform| {
            platform
                .executables
                .iter()
                .find(|executable| executable.name == "cargo-nextest")
        })
        .ok_or("qualified nextest pin missing")?;
    Ok(vec![
        (
            "bin/mise",
            official(
                SourceBuildBootstrapTool::Mise,
                SourceBuildBootstrapHost::LinuxAmd64,
            )
            .binary_sha256()
            .to_owned(),
        ),
        (
            "bin/mbx",
            official(
                SourceBuildBootstrapTool::Mbx,
                SourceBuildBootstrapHost::LinuxAmd64,
            )
            .binary_sha256()
            .to_owned(),
        ),
        ("bin/cargo-nextest", nextest.sha256.clone()),
    ])
}

pub(super) fn verify_rust_toolchain(root: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new(root.join("cargo/bin/rustc"))
        .arg("-Vv")
        .env("RUSTUP_HOME", root.join("rustup"))
        .env("CARGO_HOME", root.join("cargo"))
        .output()?;
    let version = String::from_utf8(output.stdout)?;
    if !output.status.success()
        || !version
            .lines()
            .any(|line| line == format!("release: {}", velnor_actions_mise::RUST_VERSION))
        || !version
            .lines()
            .any(|line| line == "host: x86_64-unknown-linux-gnu")
    {
        return Err(format!("unqualified Rust toolchain: {version}").into());
    }
    Ok(())
}

pub(super) fn wrapper_command(
    script: &str,
    root: &Path,
    runner: &Path,
    tools: &Path,
    job: &str,
    version: &str,
    digest: &str,
) -> Result<Command, Box<dyn Error>> {
    let path = std::env::join_paths([
        tools.join("bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ])?;
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .current_dir(root)
        .env_clear()
        .env("PATH", path)
        .env("RUNNER_TEMP", runner)
        .env("GITHUB_WORKSPACE", root)
        .env("GITHUB_JOB", job)
        .env("GITHUB_RUN_ID", "190003")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("VELNOR_RUNTIME_RUNNER_TEMP", runner)
        .env("VELNOR_GENERATOR_VERSION", version)
        .env("VELNOR_TASK_EXECUTION_DIGEST", digest)
        .env("MISE_DATA_DIR", tools.join("mise-data"))
        .env("MISE_NO_CONFIG", "1")
        .env("MISE_NO_ENV", "1")
        .env("MISE_NO_HOOKS", "1")
        .env("MISE_OFFLINE", "1")
        .env("CARGO_NET_OFFLINE", "true");
    Ok(command)
}

pub(super) fn action_script(yaml: &str) -> Result<String, Box<dyn Error>> {
    let scalar = yaml
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or("action run missing")?;
    Ok(serde_json::from_str(scalar)?)
}

#[cfg(unix)]
pub(super) fn link_dir(source: &Path, dest: &Path) -> Result<(), Box<dyn Error>> {
    std::os::unix::fs::symlink(source, dest)?;
    Ok(())
}
#[cfg(not(unix))]
pub(super) fn link_dir(_source: &Path, _dest: &Path) -> Result<(), Box<dyn Error>> {
    Err("Unix runner required".into())
}
pub(super) fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub(super) fn assert_pass(request: &Value) -> Result<(), Box<dyn Error>> {
    let out: Value = serde_json::from_str(&merge_internal_with_context(
        &request.to_string(),
        &MergeContext::detached(),
    )?)?;
    assert_eq!(out["status"], "passed", "{out}");
    Ok(())
}
pub(super) fn assert_reject(request: &Value) -> Result<(), Box<dyn Error>> {
    let out: Value = serde_json::from_str(&merge_internal_with_context(
        &request.to_string(),
        &MergeContext::detached(),
    )?)?;
    assert_ne!(out["status"], "passed", "{out}");
    Ok(())
}
pub(super) fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}
pub(super) fn sha(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 15)]));
    }
    encoded
}
pub(super) fn fixture_manifest() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = SUPPORTED_TARGETS.iter().map(|target| { let data = format!("Velnor synthetic test payload; version={version}; target={target}\n"); json!({"target": target, "artifact": format!("https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}"), "sha256": sha(data.as_bytes())}) }).collect::<Vec<_>>();
    let marker = sha(b"Velnor synthetic test source marker");
    json!({"schema": 1, "version": version, "repository": "tailrocks/velnor-new", "commit": &marker[..40], "targets": targets}).to_string()
}

pub(super) fn fixture() -> Result<(tempfile::TempDir, PathBuf, String), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join("crates/fixture/src"))?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/fixture\"]\nresolver = \"2\"\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(
        root.join("crates/fixture/Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        root.join("crates/fixture/src/lib.rs"),
        "pub fn value() -> u8 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn value_is_one() {\n        assert_eq!(super::value(), 1);\n    }\n}\n",
    )?;
    fs::write(
        root.join(".velnor/config.toml"),
        r#"schema = 2
[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "main"
[stacks.rust]
test_runner = "cargo_nextest"
[[stacks.rust.configurations]]
name = "default"
features = ["default"]
all_features = false
target = "host"
[execution]
mode = "both"
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
"#,
    )?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest(),
    )?;
    git(&root, &["init", "-b", "main"])?;
    git(&root, &["config", "user.name", "Fixture"])?;
    git(&root, &["config", "user.email", "fixture@example.invalid"])?;
    git(&root, &["add", "-A"])?;
    git(&root, &["commit", "-m", "fixture"])?;
    let head = git(&root, &["rev-parse", "HEAD"])?;
    Ok((temp, root, head))
}
