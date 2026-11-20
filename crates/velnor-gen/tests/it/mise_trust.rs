// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use crate::release_symlink::{Scratch, git};

type TestResult = Result<(), Box<dyn Error>>;

const HELPER_LOCK: &[u8] =
    include_bytes!("../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");
const MISE_TOML: &str = include_str!("../../../../generator/samples/helper-lock/view/mise.toml");
const MISE_LOCK: &[u8] = include_bytes!("../../../../generator/samples/helper-lock/view/mise.lock");
const RUST_CI: &[u8] =
    include_bytes!("../../../../generator/samples/helper-lock/view/.velnor/rust-ci.toml");

/// Stands in for the mise-managed `cargo` wrapper of a checkout whose
/// `mise.toml` declares `[wrappers.cargo]`. Like mise, it refuses to run in a
/// directory whose `mise.toml` is not under a `MISE_TRUSTED_CONFIG_PATHS`
/// entry, and otherwise answers `cargo metadata` with a canned workspace.
fn fake_cargo(dir: &Path, metadata: &Path, log: &Path) -> Result<(), Box<dyn Error>> {
    let script = format!(
        r#"#!/bin/sh
here=$(pwd -P)
if [ -f mise.toml ]; then
  trusted=0
  old_ifs=$IFS
  IFS=:
  for path in $MISE_TRUSTED_CONFIG_PATHS; do
    case "$here" in "$path"|"$path"/*) trusted=1 ;; esac
  done
  IFS=$old_ifs
  if [ "$trusted" = 0 ]; then
    echo "mise ERROR Config files in mise.toml are not trusted." >&2
    exit 1
  fi
fi
printf '%s\n' "$MISE_TRUSTED_CONFIG_PATHS" > '{log}'
cat '{metadata}'
"#,
        log = log.display(),
        metadata = metadata.display(),
    );
    let path = dir.join("cargo");
    fs::write(&path, script)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn checkout(name: &str, metadata: &Path) -> Result<Scratch, Box<dyn Error>> {
    let repo = Scratch(std::env::temp_dir().join(format!(
        "velnor-gen-mise-trust-{name}-{}",
        std::process::id()
    )));
    fs::remove_dir_all(&repo.0).ok();
    fs::create_dir_all(repo.0.join("src"))?;
    fs::create_dir_all(repo.0.join(".velnor"))?;
    git(&repo.0, &["init", "--quiet"])?;
    fs::write(
        repo.0.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(repo.0.join("src/lib.rs"), "")?;
    fs::write(repo.0.join(".velnor/rust-ci.lock"), HELPER_LOCK)?;
    fs::write(
        repo.0.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n",
    )?;
    fs::write(repo.0.join("mise.lock"), MISE_LOCK)?;
    fs::write(repo.0.join(".velnor/rust-ci.toml"), RUST_CI)?;
    fs::write(
        repo.0.join("mise.toml"),
        format!(
            "{MISE_TOML}\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = {{ MBX_CARGO_SHIM_MODE = \"1\" }}\n"
        ),
    )?;
    let root = repo.0.canonicalize()?;
    fs::write(
        metadata,
        format!(
            r#"{{"workspace_root": "{root}", "workspace_members": ["path+file://{root}#app@0.0.0"],
  "packages": [{{"name": "app", "version": "0.0.0", "manifest_path": "{root}/Cargo.toml",
    "dependencies": [], "targets": [{{"name": "app", "kind": ["lib"], "src_path": "{root}/src/lib.rs", "test": true}}]}}]}}"#,
            root = root.display()
        ),
    )?;
    Ok(repo)
}

fn velnor_gen(
    repo: &Scratch,
    bin: &Path,
    trusted: &str,
    command: &str,
) -> Result<Output, Box<dyn Error>> {
    let path = format!("{}:/usr/bin:/bin", bin.display());
    Ok(Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", command])
        .current_dir(&repo.0)
        .env("PATH", path)
        .env("MISE_TRUSTED_CONFIG_PATHS", trusted)
        .output()?)
}

#[test]
fn generate_and_check_run_cargo_metadata_in_an_untrusted_mbx_checkout_without_a_manual_trust()
-> TestResult {
    let bin = Scratch(
        std::env::temp_dir().join(format!("velnor-gen-mise-trust-bin-{}", std::process::id())),
    );
    fs::remove_dir_all(&bin.0).ok();
    fs::create_dir_all(&bin.0)?;
    let metadata = bin.0.join("metadata.json");
    let repo = checkout("untrusted", &metadata)?;
    let log = bin.0.join("trusted-paths.log");
    fake_cargo(&bin.0, &metadata, &log)?;
    git(&repo.0, &["add", "-A"])?;

    // Another checkout is already trusted: velnor-gen appends its own root
    // to that value and does not replace it.
    let elsewhere = "/elsewhere/already-trusted";
    for command in ["generate", "check"] {
        let output = velnor_gen(&repo, &bin.0, elsewhere, command)?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("not trusted"), "{command}: {stderr}");
        assert_eq!(output.status.code(), Some(0), "{command}: {stderr}");
        let seen = fs::read_to_string(&log)?;
        let root = repo.0.canonicalize()?;
        assert_eq!(
            seen.trim_end(),
            format!("{elsewhere}:{}", root.display()),
            "{command}"
        );
    }
    Ok(())
}
