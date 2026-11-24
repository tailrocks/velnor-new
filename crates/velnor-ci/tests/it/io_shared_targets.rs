// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};
use velnor_test_support::real_cargo_str;

use crate::io::Repo;

fn run(root: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{program} {args:?}: {text}");
    text
}

fn inputs(root: &Path, operation: &str, target: &str) {
    run(
        root,
        env!("CARGO_BIN_EXE_velnor-ci"),
        &["inputs", operation, "--target-dir", target],
    );
}

fn cargo(root: &Path, target: Option<&str>) -> String {
    let mut args = vec!["build", "--offline", "-v"];
    if let Some(target) = target {
        args.extend(["--target-dir", target]);
    }
    run(root, &real_cargo_str(), &args)
}

fn assert_fresh(root: &Path, name: &str, target: Option<&str>) {
    let warm = cargo(root, target);
    assert!(warm.contains(&format!("Fresh {name} ")), "{warm}");
    assert!(!warm.contains("Compiling "), "{warm}");
}

fn shared_target(configured: bool) {
    let repo = Repo::new("shared-target");
    repo.write_all(&[
        ("mise.toml", "[tools]\nrust='1.99.1'\n"),
        (".gitignore", "/shared-cache/\n"),
        (
            "a/Cargo.toml",
            "[package]\nname='shared_a'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("a/src/lib.rs", "pub fn value() -> u8 { 1 }\n"),
        (
            "b/Cargo.toml",
            "[package]\nname='shared_b'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("b/src/lib.rs", "pub fn value() -> u8 { 2 }\n"),
    ]);
    if configured {
        repo.write(".cargo/config.toml", "[build]\ntarget-dir='shared-cache'\n");
        velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let target = repo.root().join("shared-cache");
    let absolute = target.to_str().unwrap_or_else(|| unreachable!());
    let override_target = (!configured).then_some(absolute);
    for name in ["a", "b"] {
        cargo(&repo.root().join(name), override_target);
    }
    repo.git(&["add", "--all"]);
    let staged = run(repo.root(), "git", &["ls-files"]);
    assert!(!staged.contains("shared-cache/"), "{staged}");
    let built = [repo.mtime("a/src/lib.rs"), repo.mtime("b/src/lib.rs")];
    for name in ["a", "b"] {
        inputs(&repo.root().join(name), "save", "../shared-cache");
    }
    let checkout = SystemTime::now() + Duration::from_secs(10);
    for name in ["a", "b"] {
        repo.set_mtime(&format!("{name}/src/lib.rs"), checkout);
        inputs(&repo.root().join(name), "restore", absolute);
    }
    assert_eq!(repo.mtime("a/src/lib.rs"), built[0]);
    assert_eq!(repo.mtime("b/src/lib.rs"), built[1]);
    assert_fresh(&repo.root().join("a"), "shared_a", override_target);
    assert_fresh(&repo.root().join("b"), "shared_b", override_target);
    repo.write_unstaged("a/src/lib.rs", "pub fn value() -> u8 { 3 }\n");
    repo.set_mtime("a/src/lib.rs", checkout);
    for name in ["a", "b"] {
        inputs(&repo.root().join(name), "restore", "../shared-cache");
    }
    assert_eq!(repo.mtime("a/src/lib.rs"), checkout);
    let changed = cargo(&repo.root().join("a"), override_target);
    assert!(changed.contains("Compiling shared_a "), "{changed}");
    assert_fresh(&repo.root().join("b"), "shared_b", override_target);
}

#[test]
fn configured_shared_targets_restore_both_roots_and_keep_changed_inputs_dirty() {
    shared_target(true);
}

#[test]
fn explicit_shared_targets_restore_both_roots_without_recorded_metadata() {
    shared_target(false);
}
