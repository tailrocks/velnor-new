// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const EXPECTED_NEEDS: [&str; 9] = [
    "plan",
    "rust-sample-agent-status-read",
    "rust-sample-agent-status-write",
    "rust-sample-read",
    "rust-sample-write",
    "jactionlint",
    "alint",
    "cargo-deny",
    "cargo-machete",
];

#[test]
fn public_plan_and_generation_preserve_native_required_order() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().canonicalize()?.join("repo");
    create_fixture(&repo)?;
    let cli = env!("CARGO_BIN_EXE_velnor-actions");

    let plan = invoke(cli, &repo, &["plan"])?;
    assert_success(&plan, "public plan");
    assert!(
        String::from_utf8(plan.stdout)?.contains("Workflow to generate"),
        "public plan omitted its workflow report"
    );

    let first = temp.path().join("first");
    let second = temp.path().join("second");
    for output_dir in [&first, &second] {
        let output = invoke(
            cli,
            &repo,
            &[
                "generate",
                "--output-dir",
                output_dir.to_str().ok_or("non-UTF-8 output path")?,
            ],
        )?;
        assert_success(&output, "public generation");
    }
    assert_eq!(
        tree(&first.join(".github"))?,
        tree(&second.join(".github"))?
    );

    let workflow = fs::read_to_string(first.join(".github/workflows/ci.yml"))?;
    assert_eq!(required_needs(&workflow)?, EXPECTED_NEEDS);
    Ok(())
}

fn create_fixture(repo: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(repo.join(".velnor"))?;
    fs::write(repo.join("Cargo.toml"), CARGO_WORKSPACE)?;
    for (name, manifest) in [
        ("sample", SAMPLE_MANIFEST),
        ("sample-agent-status", SAMPLE_AGENT_STATUS_MANIFEST),
    ] {
        let package = repo.join("crates").join(name);
        fs::create_dir_all(package.join("src"))?;
        fs::write(package.join("Cargo.toml"), manifest)?;
        fs::write(package.join("src/lib.rs"), "pub fn fixture() {}\n")?;
    }
    fs::write(repo.join(".velnor/config.toml"), CONFIG)?;
    for (path, contents) in [
        (
            ".velnor/generator.lock",
            include_str!("../../../../.velnor/generator.lock"),
        ),
        (
            ".velnor/mise.toml",
            include_str!("../../../../.velnor/mise.toml"),
        ),
        (
            ".velnor/mise.lock",
            include_str!("../../../../.velnor/mise.lock"),
        ),
        (
            ".velnor/rust-toolchain.toml",
            include_str!("../../../../.velnor/rust-toolchain.toml"),
        ),
        (
            "rust-toolchain.toml",
            include_str!("../../../../rust-toolchain.toml"),
        ),
    ] {
        fs::write(repo.join(path), contents)?;
    }
    fs::create_dir_all(repo.join("home"))?;
    write_tool_transport(repo)?;
    git(repo, &["init", "-b", "main"])?;
    git(
        repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
    )?;
    git(repo, &["add", "."])?;
    git(
        repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "fixture",
        ],
    )?;
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let path = std::env::var_os("PATH").ok_or("test PATH is missing")?;
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env_clear()
        .env("PATH", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("HOME", repo.join("home"))
        .output()?;
    assert_success(&output, "Git fixture setup");
    Ok(())
}

fn invoke(cli: &str, repo: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let path = std::env::join_paths([
        repo.join("bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ])?;
    Ok(Command::new(cli)
        .args(args)
        .current_dir(repo)
        .env_clear()
        .env("PATH", path)
        .env("HOME", repo.join("home"))
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?)
}

fn write_tool_transport(repo: &Path) -> Result<(), Box<dyn Error>> {
    use serde_json::json;
    use velnor_actions_mise::{PinnedTool, ToolCatalog};

    let bin = repo.join("bin");
    fs::create_dir_all(&bin)?;
    let packages = ["sample", "sample-agent-status"]
        .map(|name| {
            json!({
                "id": format!("path+file://{}/crates/{name}#0.1.0", repo.display()),
                "name": name,
                "version": "0.1.0",
                "manifest_path": repo.join("crates").join(name).join("Cargo.toml"),
                "targets": [{"kind": ["lib"], "name": name.replace('-', "_"), "test": true, "doctest": true}],
                "dependencies": [],
                "features": {},
            })
        });
    let members = packages
        .iter()
        .map(|package| package["id"].clone())
        .collect::<Vec<_>>();
    let metadata = json!({
        "version": 1,
        "workspace_root": repo,
        "workspace_members": members,
        "packages": packages,
    });
    fs::write(bin.join("metadata.json"), serde_json::to_vec(&metadata)?)?;
    let catalog = ToolCatalog::pinned();
    let script = TOOL_TRANSPORT
        .replace("@RUST_VERSION@", catalog.version(PinnedTool::Rust))
        .replace("@RUST_SPEC@", &catalog.tool_spec(PinnedTool::Rust))
        .replace(
            "@JACTIONLINT_SPEC@",
            &catalog.tool_spec(PinnedTool::Jactionlint),
        )
        .replace(
            "@SHELLCHECK_SPEC@",
            &catalog.tool_spec(PinnedTool::Shellcheck),
        )
        .replace(
            "@SHELLCHECK_VERSION@",
            catalog.version(PinnedTool::Shellcheck),
        );
    let mise = bin.join("mise");
    fs::write(&mise, script)?;
    fs::set_permissions(mise, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

const TOOL_TRANSPORT: &str = r#"#!/bin/sh
set -eu
[ "$#" -ge 4 ] || exit 90
[ "$1" = "--no-config" ] && [ "$2" = "--no-env" ] && [ "$3" = "--no-hooks" ] || exit 91
shift 3
if [ "$1" = "ls" ]; then
  [ "$#" -eq 3 ] && [ "$2" = "--json" ] && [ "$3" = "rust" ] || exit 92
  printf '[{"version":"@RUST_VERSION@","installed":true}]\n'
  exit 0
fi
[ "$1" = "exec" ] || exit 93
shift
if [ "$1" = "@RUST_SPEC@" ]; then
  shift
  [ "$#" -eq 8 ] && [ "$1" = "--" ] && [ "$2" = "cargo" ] && [ "$3" = "metadata" ] || exit 94
  [ "$4" = "--format-version" ] && [ "$5" = "1" ] && [ "$6" = "--no-deps" ] && [ "$7" = "--manifest-path" ] || exit 95
  [ "$8" = "$(pwd -P)/Cargo.toml" ] || exit 96
  cat "$(dirname "$0")/metadata.json"
  exit 0
fi
if [ "$1" = "@JACTIONLINT_SPEC@" ]; then
  shift
  [ "$1" = "@SHELLCHECK_SPEC@" ] || exit 97
  shift
  [ "$1" = "--" ] && [ "$2" = "jactionlint" ] || exit 98
  shift 2
  [ "$1" = "--config-file" ] && [ "$2" = ".github/jactionlint.yaml" ] || exit 99
  [ "$3" = "--format" ] && [ "$4" = "oneline" ] || exit 100
  shift 4
  for flag in --strict-exit --no-online --shellcheck=shellcheck --pyflakes= --no-color; do
    [ "$1" = "$flag" ] || exit 101
    shift
  done
else
  [ "$1" = "@SHELLCHECK_SPEC@" ] || exit 102
  shift
  [ "$1" = "--" ] && [ "$2" = "shellcheck" ] || exit 103
  shift 2
  if [ "$1" = "--version" ]; then
    [ "$#" -eq 1 ] || exit 104
    printf 'version: @SHELLCHECK_VERSION@\n'
    exit 0
  fi
  if [ "$1" = "-S" ]; then
    [ "$2" = "warning" ] && [ "$3" = "--format=gcc" ] || exit 105
    shift 3
  else
    [ "$1" = "--format=gcc" ] && [ "$2" = "--include=SC2086" ] || exit 106
    shift 2
  fi
fi
[ "$#" -gt 0 ] || exit 107
for file in "$@"; do [ -f "$file" ] || exit 108; done
"#;

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn required_needs(workflow: &str) -> Result<Vec<&str>, Box<dyn Error>> {
    let mut in_required = false;
    let mut in_needs = false;
    let mut needs = Vec::new();
    for line in workflow.lines() {
        if line == "  required:" {
            in_required = true;
            continue;
        }
        if in_required && line.starts_with("  ") && !line.starts_with("    ") {
            break;
        }
        if in_required && line == "    needs:" {
            in_needs = true;
            continue;
        }
        if in_needs && line.starts_with("      - ") {
            needs.push(line.trim_start_matches("      - "));
        } else if in_needs && !line.starts_with("      ") {
            break;
        }
    }
    if !in_needs {
        return Err("Required needs section missing".into());
    }
    Ok(needs)
}

type GeneratedTree = Vec<(PathBuf, Vec<u8>)>;

fn tree(root: &Path) -> Result<GeneratedTree, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut GeneratedTree,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_files(root, &path, files)?;
        } else {
            files.push((path.strip_prefix(root)?.to_path_buf(), fs::read(path)?));
        }
    }
    Ok(())
}

const CARGO_WORKSPACE: &str = "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n";

const SAMPLE_MANIFEST: &str =
    "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

const SAMPLE_AGENT_STATUS_MANIFEST: &str =
    "[package]\nname = \"sample-agent-status\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

const CONFIG: &str = "schema = 2\n\n[workflow]\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"main\"\ngenerator_validation = \"bootstrap\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n\n[agents]\nclaude_md = \"import\"\n";
