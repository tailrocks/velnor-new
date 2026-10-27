// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use velnor_actions_contract::SUPPORTED_TARGETS;

#[path = "../src/git_fixture.rs"]
pub(crate) mod git_fixture;

pub(crate) fn make_repo(config: &str) -> Result<TempDir, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

pub(crate) fn config_with_verification_branch(branch: Option<&str>) -> String {
    let verification_branch = branch.map_or_else(String::new, |branch| {
        format!("verification_branch = \"{branch}\"\n")
    });
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{verification_branch}"
    )
}

pub(crate) fn add_independent_mbx_package(
    root: &Path,
    relative_root: &str,
    package_name: &str,
) -> Result<(), Box<dyn Error>> {
    let package_root = root.join(relative_root);
    fs::create_dir_all(package_root.join("src"))?;
    fs::write(
        package_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n"
        ),
    )?;
    fs::write(package_root.join("src/lib.rs"), "pub fn f() {}\n")?;
    fs::write(
        package_root.join("Cargo.lock"),
        format!("version = 4\n\n[[package]]\nname = \"{package_name}\"\nversion = \"0.1.0\"\n"),
    )?;
    fs::write(
        package_root.join("mise.toml"),
        "wrappers = { cargo = { command = \"mbx\", env = { MBX_CARGO_SHIM_MODE = \"1\" } } }\n",
    )?;
    Ok(())
}

pub(crate) fn write_nextest_task(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(root.join(".mise/tasks"))?;
    let task = root.join(".mise/tasks/test");
    fs::write(&task, "#!/bin/sh\ncargo nextest run --locked\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&task)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&task, permissions)?;
    }
    Ok(())
}

fn fixture_manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            let payload = format!(
                "Velnor synthetic test payload; version={version}; target={target}\n"
            );
            let digest = synthetic_sha256(payload.as_bytes());
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{digest}\"}}"
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let source_marker = synthetic_sha256(b"Velnor synthetic test source marker");
    let commit = source_marker.chars().take(40).collect::<String>();
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
    )
}

fn synthetic_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub(crate) fn git(args: &[&str], cwd: &Path) -> Result<(), Box<dyn Error>> {
    let output = git_fixture::command(cwd)?
        .args(args)
        .current_dir(cwd)
        .output()?;
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
