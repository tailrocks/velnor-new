// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;

use super::{command, unit, velnor_ci};
use crate::io::Repo;

/// A repository root with its own package, and a nested Cargo root `lab`
/// beneath it. Neither is in a workspace group.
fn nested_roots() -> Repo {
    let repo = Repo::new("audit-nested-roots");
    unit(&repo, ".gitignore", "/target/\n/lab/target/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    for (directory, name) in [("", "top"), ("lab/", "lab")] {
        unit(
            &repo,
            &format!("{directory}Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        unit(&repo, &format!("{directory}Cargo.lock"), "version = 4\n");
        unit(&repo, &format!("{directory}src/lib.rs"), "");
    }
    unit(
        &repo,
        "driver/cargo.rs",
        "fn main() {\n    let args: Vec<String> = std::env::args().skip(1).collect();\n    if args.first().is_some_and(|arg| arg == \"metadata\") {\n        std::process::exit(std::process::Command::new(\"cargo\").args(&args).status().unwrap().code().unwrap_or(1));\n    }\n}\n",
    );
    let compiled = command("rustc")
        .current_dir(repo.root())
        .args(["--edition=2024", "driver/cargo.rs", "-o", "driver/cargo"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(compiled.status.success(), "{compiled:?}");
    repo.git(&["add", "--all"]);
    repo
}

/// One job step of the audit: `velnor-ci inputs build` in `directory`, with
/// the step's own `CARGO_TARGET_DIR`, as the generated workflow sets it.
fn build_step(repo: &Repo, directory: &str, target: &str) -> Output {
    command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join(directory))
        .env("CARGO_TARGET_DIR", repo.root().join(target))
        .args(["inputs", "build", "--"])
        .arg(repo.root().join("driver/cargo"))
        .args(["build", "--workspace"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn an_audit_of_a_nested_root_does_not_refuse_the_target_of_the_root_above() {
    let repo = nested_roots();
    let output = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", repo.root().join("lab/target"))
        .args(["inputs", "audit", "--root", "lab"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains("cannot be told apart"), "{text}");
}

#[test]
fn each_nested_root_step_builds_with_its_own_target_directory() {
    let repo = nested_roots();
    for (directory, target) in [("", "target"), ("lab", "lab/target")] {
        let built = build_step(&repo, directory, target);
        let text = String::from_utf8_lossy(&built.stderr);
        assert!(
            built.status.success(),
            "step in {directory:?} with {target}: {text}"
        );
    }
}

#[test]
fn a_nested_root_selected_by_manifest_path_builds_from_the_repository_root() {
    let repo = nested_roots();
    let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", repo.root().join("lab/target"))
        .args(["inputs", "build", "--"])
        .arg(repo.root().join("driver/cargo"))
        .args(["build", "--manifest-path", "lab/Cargo.toml"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    let text = String::from_utf8_lossy(&built.stderr);
    assert!(built.status.success(), "{text}");
}
